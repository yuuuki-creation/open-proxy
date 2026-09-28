//! 证书：主控统一用 DNS 验证（Cloudflare）向 Let's Encrypt 申请和续期（nodes.md「证书」）。
//! 申请主控自己的 HTTPS 证书，和证书方式是 acme 的服务器的证书；到期前 30 天续期。
//! DNS API 凭据只放在主控，节点服务器被入侵也拿不到。

mod cloudflare;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus,
};
use sha2::{Digest, Sha256};

use crate::app::AppState;
use crate::db::{self, servers::NewCertificate, settings};
use crate::tls;

/// 到期前多少天续期
const RENEW_BEFORE_DAYS: i64 = 30;
/// 同一个域名失败后，至少隔这么久再试（Let's Encrypt 有失败次数限制）
const RETRY_AFTER: Duration = Duration::from_secs(3600);

static LAST_ATTEMPT: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 申请到的证书。
pub struct Issued {
    pub cert_pem: String,
    pub key_pem: String,
    pub sha256: String,
    pub not_after: i64,
}

/// 检查所有需要自动申请的证书：没有、域名变了或快到期的就申请。定时任务里调用。
pub async fn check_all(state: &AppState, staging: bool) {
    let token: Option<String> = settings::get(&state.db, settings::CLOUDFLARE_API_TOKEN)
        .await
        .ok()
        .flatten()
        .filter(|t: &String| !t.is_empty());
    let Some(token) = token else {
        return;
    };

    // 主控自己的证书
    let domain: Option<String> = settings::get(&state.db, settings::DOMAIN)
        .await
        .ok()
        .flatten()
        .filter(|d: &String| !d.is_empty());
    if let Some(domain) = domain {
        match need_issue(state, None, &domain).await {
            Ok(true) => {
                if let Some(issued) = attempt(state, &token, &domain, staging).await {
                    match tls::certified_key(&issued.cert_pem, &issued.key_pem) {
                        Ok(key) => {
                            if save(state, None, &domain, &issued).await {
                                state.certs.replace(key);
                                tracing::info!(%domain, "主控 HTTPS 证书已更新");
                            }
                        }
                        Err(err) => tracing::error!("新申请的主控证书用不了: {err:#}"),
                    }
                }
            }
            Ok(false) => {}
            Err(err) => tracing::warn!("检查主控证书出错: {err:#}"),
        }
    }

    // 证书方式是 acme 的服务器
    let servers = match db::servers::list(&state.db).await {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!("读取服务器列表出错: {err}");
            return;
        }
    };
    for server in servers {
        let (Some(domain), "acme") = (server.cert_domain.clone(), server.cert_mode.as_str()) else {
            continue;
        };
        match need_issue(state, Some(server.id), &domain).await {
            Ok(true) => {
                if let Some(issued) = attempt(state, &token, &domain, staging).await
                    && save(state, Some(server.id), &domain, &issued).await
                {
                    tracing::info!(server_id = server.id, %domain, "服务器证书已更新，推送新的期望状态");
                    state.config_changed();
                }
            }
            Ok(false) => {}
            Err(err) => tracing::warn!(server_id = server.id, "检查服务器证书出错: {err:#}"),
        }
    }
}

/// 要不要申请：没有证书、不是这个域名的自动申请证书、或者 30 天内到期。
async fn need_issue(
    state: &AppState,
    server_id: Option<i64>,
    domain: &str,
) -> anyhow::Result<bool> {
    let cert = db::servers::certificate(&state.db, server_id).await?;
    Ok(match cert {
        None => true,
        Some(c) => {
            c.kind != "acme"
                || c.domain.as_deref() != Some(domain)
                || c.not_after - db::now_ms() < RENEW_BEFORE_DAYS * 86_400_000
        }
    })
}

/// 申请一次，失败时记日志并限流；成功返回证书。
async fn attempt(state: &AppState, token: &str, domain: &str, staging: bool) -> Option<Issued> {
    {
        let mut last = LAST_ATTEMPT.lock().unwrap_or_else(|e| e.into_inner());
        if last.get(domain).is_some_and(|t| t.elapsed() < RETRY_AFTER) {
            return None;
        }
        last.insert(domain.to_string(), Instant::now());
    }
    tracing::info!(%domain, staging, "开始申请证书（DNS 验证）");
    match issue(state, token, domain, staging).await {
        Ok(issued) => {
            LAST_ATTEMPT
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(domain);
            Some(issued)
        }
        Err(err) => {
            tracing::error!(%domain, "申请证书失败，一小时后再试: {err:#}");
            let _ = record_error(state, domain, &format!("{err:#}")).await;
            None
        }
    }
}

async fn save(state: &AppState, server_id: Option<i64>, domain: &str, issued: &Issued) -> bool {
    let result = db::servers::save_certificate(
        &state.db,
        &NewCertificate {
            server_id,
            kind: "acme",
            domain: Some(domain),
            cert_pem: &issued.cert_pem,
            key_pem: &issued.key_pem,
            sha256: &issued.sha256,
            not_after: issued.not_after,
        },
    )
    .await;
    if let Err(err) = &result {
        tracing::error!(%domain, "保存证书出错: {err}");
    }
    result.is_ok()
}

/// 把失败原因记到这个域名现有的证书上（面板显示）。
async fn record_error(state: &AppState, domain: &str, error: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE certificates SET last_error = ? WHERE domain = ?")
        .bind(error)
        .bind(domain)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// ACME 账户：第一次用时创建，凭据存在设置里。
async fn account(state: &AppState, staging: bool) -> anyhow::Result<Account> {
    let key = if staging {
        "acme_account_staging"
    } else {
        settings::ACME_ACCOUNT
    };
    if let Some(creds) = settings::get::<AccountCredentials>(&state.db, key).await? {
        return Account::from_credentials(creds)
            .await
            .context("恢复 ACME 账户");
    }
    let url = if staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };
    let (account, creds) = Account::create(
        &NewAccount {
            contact: &[],
            terms_of_service_agreed: true,
            only_return_existing: false,
        },
        url,
        None,
    )
    .await
    .context("创建 ACME 账户")?;
    settings::set(&state.db, key, &creds).await?;
    Ok(account)
}

/// 用 DNS-01 申请一张证书。TXT 记录用完一定删掉。
async fn issue(
    state: &AppState,
    token: &str,
    domain: &str,
    staging: bool,
) -> anyhow::Result<Issued> {
    let account = account(state, staging).await?;
    let identifiers = [Identifier::Dns(domain.to_string())];
    let mut order = account
        .new_order(&NewOrder {
            identifiers: &identifiers,
        })
        .await
        .context("创建 ACME 订单")?;

    let cf = cloudflare::Client::new(token)?;
    let mut records = Vec::new();
    let result = async {
        let mut challenges = Vec::new();
        for authz in order.authorizations().await.context("读取授权")? {
            if authz.status == AuthorizationStatus::Valid {
                continue;
            }
            let Identifier::Dns(name) = &authz.identifier;
            let challenge = authz
                .challenges
                .iter()
                .find(|c| c.r#type == ChallengeType::Dns01)
                .ok_or_else(|| anyhow!("CA 没有提供 DNS 验证"))?;
            let value = order.key_authorization(challenge).dns_value();
            let record_name = format!("_acme-challenge.{}", name.trim_start_matches("*."));
            records.push(cf.create_txt(&record_name, &value).await?);
            challenges.push(challenge.url.clone());
        }
        // 等 DNS 记录生效
        tokio::time::sleep(Duration::from_secs(30)).await;
        for url in &challenges {
            order
                .set_challenge_ready(url)
                .await
                .context("通知 CA 验证")?;
        }
        let mut delay = Duration::from_secs(3);
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            tokio::time::sleep(delay).await;
            let state = order.refresh().await.context("查询订单状态")?;
            match state.status {
                OrderStatus::Ready | OrderStatus::Valid => break,
                OrderStatus::Invalid => bail!("CA 验证失败: {:?}", state.error),
                _ if Instant::now() > deadline => bail!("等 CA 验证超时"),
                _ => delay = (delay * 2).min(Duration::from_secs(15)),
            }
        }

        let key_pair = rcgen::KeyPair::generate().context("生成私钥")?;
        let params = rcgen::CertificateParams::new(vec![domain.to_string()]).context("证书参数")?;
        let csr = params.serialize_request(&key_pair).context("生成 CSR")?;
        if order.state().status != OrderStatus::Valid {
            order
                .finalize(csr.der().as_ref())
                .await
                .context("提交 CSR")?;
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        let cert_pem = loop {
            if let Some(pem) = order.certificate().await.context("下载证书")? {
                break pem;
            }
            if Instant::now() > deadline {
                bail!("等证书签发超时");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        };
        let (sha256, not_after) = leaf_info(&cert_pem)?;
        Ok(Issued {
            cert_pem,
            key_pem: key_pair.serialize_pem(),
            sha256,
            not_after,
        })
    }
    .await;

    for record in records {
        if let Err(err) = cf.delete(&record).await {
            tracing::warn!(%domain, "删除验证用的 TXT 记录失败，请手动删除: {err:#}");
        }
    }
    result
}

/// 证书链里第一张（服务器证书）的 SHA-256 和到期时间（Unix 毫秒）。
fn leaf_info(cert_pem: &str) -> anyhow::Result<(String, i64)> {
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    let leaf = CertificateDer::pem_slice_iter(cert_pem.as_bytes())
        .next()
        .ok_or_else(|| anyhow!("证书链是空的"))?
        .context("解析证书")?;
    let (_, parsed) =
        x509_parser::parse_x509_certificate(leaf.as_ref()).map_err(|e| anyhow!("解析证书: {e}"))?;
    let not_after = parsed.validity().not_after.timestamp() * 1000;
    Ok((hex::encode(Sha256::digest(leaf.as_ref())), not_after))
}
