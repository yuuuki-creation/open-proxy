//! 主控自己的 HTTPS。证书从数据库读（server_id 为空的那一张）；还没有时用启动时生成的临时自签证书。
//! 申请到正式证书后调用 `CertStore::replace` 热替换，不用重启（P5）。

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use chrono::{Datelike, Utc};
use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// 当前使用的证书，可以随时替换。
#[derive(Debug)]
pub struct CertStore {
    current: RwLock<Arc<CertifiedKey>>,
}

impl CertStore {
    /// 从数据库读主控证书；没有或读不出来时生成临时自签证书。
    pub async fn load(db: &SqlitePool) -> anyhow::Result<Arc<Self>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT cert_pem, key_pem FROM certificates WHERE server_id IS NULL")
                .fetch_optional(db)
                .await
                .context("读取主控证书")?;
        let key = match row {
            Some((cert_pem, key_pem)) => match certified_key(&cert_pem, &key_pem) {
                Ok(key) => key,
                Err(err) => {
                    tracing::warn!("主控证书用不了，先用临时自签证书: {err:#}");
                    temporary()?
                }
            },
            None => {
                tracing::info!("还没有主控证书，先用临时自签证书");
                temporary()?
            }
        };
        Ok(Arc::new(Self {
            current: RwLock::new(Arc::new(key)),
        }))
    }

    /// 换成新证书，之后的握手立即使用。
    pub fn replace(&self, key: CertifiedKey) {
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(key);
    }
}

impl ResolvesServerCert for CertStore {
    fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(
            self.current
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        )
    }
}

/// 生成 rustls 的服务端配置。只用 HTTP/1.1：WebSocket 和面板都不需要 HTTP/2。
pub fn server_config(store: Arc<CertStore>) -> anyhow::Result<Arc<ServerConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("TLS 协议版本")?
        .with_no_client_auth()
        .with_cert_resolver(store);
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// 从 PEM 格式的证书链和私钥构造 rustls 用的证书。
pub fn certified_key(cert_pem: &str, key_pem: &str) -> anyhow::Result<CertifiedKey> {
    let certs = CertificateDer::pem_slice_iter(cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .context("解析证书")?;
    if certs.is_empty() {
        bail!("PEM 里没有证书");
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).context("解析私钥")?;
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|e| anyhow!("不支持的私钥类型: {e}"))?;
    Ok(CertifiedKey::new(certs, signing_key))
}

/// 生成的自签证书。
#[allow(dead_code)] // sha256、not_after 在 P5 给服务器生成证书时用
pub struct SelfSigned {
    pub cert_pem: String,
    pub key_pem: String,
    /// 证书 DER 的 SHA-256，小写十六进制；自签证书的指纹写进订阅
    pub sha256: String,
    /// 到期时间，Unix 毫秒
    pub not_after: i64,
}

/// 生成自签证书（ECDSA P-256），有效期 `days` 天。
pub fn generate_self_signed(names: Vec<String>, days: i64) -> anyhow::Result<SelfSigned> {
    let key_pair = rcgen::KeyPair::generate().context("生成私钥")?;
    let mut params = rcgen::CertificateParams::new(names).context("证书参数")?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "open-proxy");
    params.distinguished_name = dn;
    let from = Utc::now() - chrono::Duration::days(1);
    let until = Utc::now() + chrono::Duration::days(days);
    params.not_before = rcgen::date_time_ymd(from.year(), from.month() as u8, from.day() as u8);
    params.not_after = rcgen::date_time_ymd(until.year(), until.month() as u8, until.day() as u8);
    let cert = params.self_signed(&key_pair).context("签发自签证书")?;
    Ok(SelfSigned {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
        sha256: hex::encode(Sha256::digest(cert.der().as_ref())),
        not_after: until.timestamp_millis(),
    })
}

fn temporary() -> anyhow::Result<CertifiedKey> {
    let generated = generate_self_signed(vec!["op-master".to_string()], 365)?;
    certified_key(&generated.cert_pem, &generated.key_pem)
}

/// HTTPS 监听器：TLS 握手放在单独的任务里做，慢的握手不会挡住别的连接。
pub struct TlsListener {
    accepted: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local_addr: SocketAddr,
}

impl TlsListener {
    pub async fn bind(addr: SocketAddr, config: Arc<ServerConfig>) -> io::Result<Self> {
        let tcp = TcpListener::bind(addr).await?;
        let local_addr = tcp.local_addr()?;
        let acceptor = TlsAcceptor::from(config);
        let (tx, accepted) = mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                let (stream, peer) = match tcp.accept().await {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!("接受连接失败: {err}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    match tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream))
                        .await
                    {
                        Ok(Ok(tls)) => {
                            let _ = tx.send((tls, peer)).await;
                        }
                        Ok(Err(err)) => tracing::debug!(%peer, "TLS 握手失败: {err}"),
                        Err(_) => tracing::debug!(%peer, "TLS 握手超时"),
                    }
                });
            }
        });
        Ok(Self {
            accepted,
            local_addr,
        })
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.accepted.recv().await {
            Some(conn) => conn,
            // 接受连接的任务不会结束；万一结束了就不再接受新连接
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}
