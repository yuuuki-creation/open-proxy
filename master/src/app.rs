//! 共享状态和总路由。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::connect_info::Connected;
use axum::serve::IncomingStream;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::api::auth::LoginLimiter;
use crate::db::settings;
use crate::{agent, agent_dist, api, state, subscription, tls, web};

/// 所有请求处理函数共享的状态。
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    /// 主控自己的 HTTPS 证书，申请到正式证书后热替换
    pub certs: Arc<tls::CertStore>,
    /// 配置或停用状态可能变了：后台任务据此重算停用状态和期望状态（P3）
    pub changes: Arc<Notify>,
    pub login_limiter: Arc<LoginLimiter>,
    /// 在线的 Agent 连接
    pub hub: Arc<agent::Hub>,
    /// 各服务器当前的期望状态
    pub states: Arc<state::Engine>,
    /// 主控托管的 Agent 二进制
    pub agent_dist: Arc<agent_dist::AgentDist>,
    /// 数据目录（数据库、备份的临时文件）
    pub data_dir: std::path::PathBuf,
    /// 用 Let's Encrypt 的测试环境
    pub acme_staging: bool,
    /// 命令行指定的对外地址，优先于设置里的域名（本机测试用）
    public_url: Option<String>,
}

impl AppState {
    pub fn new(
        db: SqlitePool,
        certs: Arc<tls::CertStore>,
        public_url: Option<String>,
        agent_dir: Option<std::path::PathBuf>,
        data_dir: std::path::PathBuf,
        acme_staging: bool,
    ) -> Self {
        Self {
            db,
            certs,
            changes: Arc::new(Notify::new()),
            login_limiter: Arc::new(LoginLimiter::default()),
            hub: Arc::new(agent::Hub::default()),
            states: Arc::new(state::Engine::default()),
            agent_dist: Arc::new(agent_dist::AgentDist::new(agent_dir)),
            data_dir,
            acme_staging,
            public_url: public_url.map(|u| u.trim_end_matches('/').to_string()),
        }
    }

    /// 任何配置变更之后调用：用户、套餐、节点、出口、服务器、设置。
    /// 后台任务把短时间内的多次通知合并成一次处理，不用判断影响了哪些服务器（database.md）。
    pub fn config_changed(&self) {
        self.changes.notify_one();
    }

    /// 马上检查一次需要自动申请的证书（改了域名、Cloudflare Token、服务器证书方式之后调用）。
    pub fn check_certificates(&self) {
        let state = self.clone();
        tokio::spawn(async move { crate::acme::check_all(&state, state.acme_staging).await });
    }

    /// 主控对外的地址（不带结尾的斜杠），拼安装命令、订阅链接用；还没有域名时为 None。
    pub async fn base_url(&self) -> anyhow::Result<Option<String>> {
        if let Some(url) = &self.public_url {
            return Ok(Some(url.clone()));
        }
        let domain: Option<String> = settings::get(&self.db, settings::DOMAIN).await?;
        Ok(domain
            .filter(|d| !d.is_empty())
            .map(|d| format!("https://{d}")))
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest("/api", api::router())
        .route("/s/{token}", axum::routing::get(subscription::serve))
        .fallback(web::serve)
        .with_state(state)
}

/// 连接的对端地址。放在反向代理后面时是代理的地址，取真实 IP 见 `api::client_ip`。
#[derive(Clone, Copy, Debug)]
pub struct PeerAddr(pub SocketAddr);

impl Connected<IncomingStream<'_, TcpListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        PeerAddr(*stream.remote_addr())
    }
}

impl Connected<IncomingStream<'_, tls::TlsListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, tls::TlsListener>) -> Self {
        PeerAddr(*stream.remote_addr())
    }
}
