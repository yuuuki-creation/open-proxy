//! op-master：open-proxy 的主控。管理面板、订阅、Agent 网关都在这一个进程里。
//! 设计见 main 分支的 docs/design-docs/，执行计划见 docs/exec-plans/active/2026-09-27-master.md。

mod acme;
mod agent;
mod agent_dist;
mod api;
mod app;
mod db;
mod jobs;
mod pb;
mod secret;
mod state;
mod subscription;
mod tls;
mod traffic;
mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

use crate::app::PeerAddr;

/// 版本号：发布时由编译环境变量 OP_VERSION 注入。和 Agent 版本一致才同步期望状态。
pub const VERSION: &str = match option_env!("OP_VERSION") {
    Some(v) => v,
    None => "dev",
};

#[derive(Parser, Debug)]
#[command(name = "op-master", version = VERSION, about = "open-proxy 主控")]
struct Cli {
    /// 数据目录，放数据库等
    #[arg(long, env = "OP_MASTER_DATA_DIR", default_value = "/var/lib/op-master")]
    data_dir: PathBuf,

    /// HTTPS 监听地址
    #[arg(long, env = "OP_MASTER_LISTEN", default_value = "0.0.0.0:443")]
    listen: SocketAddr,

    /// 不监听 HTTPS（放在反向代理后面时用，要同时指定 --http-listen）
    #[arg(long, env = "OP_MASTER_NO_HTTPS")]
    no_https: bool,

    /// 另外监听一个明文 HTTP 地址，给反向代理或本机测试用，例如 127.0.0.1:8080
    #[arg(long, env = "OP_MASTER_HTTP_LISTEN")]
    http_listen: Option<SocketAddr>,

    /// 对外地址，覆盖设置里的域名，拼安装命令和订阅链接用（本机测试时例如 http://127.0.0.1:8080）
    #[arg(long, env = "OP_MASTER_PUBLIC_URL")]
    public_url: Option<String>,

    /// 从这个目录读 Agent 二进制和签名，不用嵌入的（测试、Docker 用）
    #[arg(long, env = "OP_MASTER_AGENT_DIR")]
    agent_dir: Option<PathBuf>,

    /// 用 Let's Encrypt 的测试环境申请证书（签出来的证书浏览器不信任，只用来调试申请流程）
    #[arg(long, env = "OP_MASTER_ACME_STAGING")]
    acme_staging: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        // 输出到 journald 等非终端时不带颜色控制符
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    if let Err(err) = run(Cli::parse()).await {
        tracing::error!("退出: {err:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    if cli.no_https && cli.http_listen.is_none() {
        anyhow::bail!("关闭 HTTPS 时要用 --http-listen 指定明文 HTTP 的监听地址");
    }
    tracing::info!(version = VERSION, data_dir = %cli.data_dir.display(), "启动");

    std::fs::create_dir_all(&cli.data_dir)
        .with_context(|| format!("创建数据目录 {}", cli.data_dir.display()))?;
    api::backup::apply_pending(&cli.data_dir).context("换上恢复的数据库")?;
    let pool = db::open(&cli.data_dir.join("op-master.db")).await?;
    let certs = tls::CertStore::load(&pool).await?;
    let state = app::AppState::new(
        pool,
        certs.clone(),
        cli.public_url.clone(),
        cli.agent_dir.clone(),
        cli.data_dir.clone(),
        cli.acme_staging,
    );
    jobs::spawn(&state);
    let router = app::router(state);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut servers = JoinSet::new();

    if !cli.no_https {
        let listener = tls::TlsListener::bind(cli.listen, tls::server_config(certs)?)
            .await
            .with_context(|| format!("监听 HTTPS {}", cli.listen))?;
        tracing::info!(addr = %cli.listen, "HTTPS 已开始监听");
        let service = router
            .clone()
            .into_make_service_with_connect_info::<PeerAddr>();
        let rx = shutdown_rx.clone();
        servers.spawn(async move {
            axum::serve(listener, service)
                .with_graceful_shutdown(wait_shutdown(rx))
                .await
        });
    }
    if let Some(addr) = cli.http_listen {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("监听 HTTP {addr}"))?;
        tracing::info!(%addr, "明文 HTTP 已开始监听");
        let service = router
            .clone()
            .into_make_service_with_connect_info::<PeerAddr>();
        let rx = shutdown_rx.clone();
        servers.spawn(async move {
            axum::serve(listener, service)
                .with_graceful_shutdown(wait_shutdown(rx))
                .await
        });
    }

    tokio::select! {
        _ = shutdown_signal() => tracing::info!("收到退出信号，停止"),
        Some(res) = servers.join_next() => {
            // 监听意外结束：直接退出，由 systemd 或 Docker 重启
            res.context("监听任务异常结束")?.context("监听出错")?;
            anyhow::bail!("监听意外结束");
        }
    }

    // 最多等 10 秒让进行中的请求结束；WebSocket 等长连接不等
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while servers.join_next().await.is_some() {}
    })
    .await;
    Ok(())
}

async fn wait_shutdown(mut rx: watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            return;
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!("注册 SIGTERM 失败，只响应 Ctrl-C: {err}");
            let _ = ctrl_c.await;
            return;
        }
    };
    tokio::select! {
        _ = ctrl_c => {}
        _ = term.recv() => {}
    }
}
