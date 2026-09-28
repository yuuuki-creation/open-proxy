//! 一条 Agent 连接：Hello 认证、收发消息、心跳（protocol.md「连接流程」）。

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::Response;
use bytes::Bytes;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use prost::Message as _;
use tokio::sync::mpsc;

use super::AgentHandle;
use crate::app::{AppState, PeerAddr};
use crate::db::{self, servers::Server};
use crate::pb::agentv1 as pb;
use crate::{VERSION, secret, traffic};

use pb::agent_message::Body as AgentBody;
use pb::hello_result::Status;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const PING_INTERVAL: Duration = Duration::from_secs(30);
/// 这么久没收到任何帧（包括 pong）就判定离线
const OFFLINE_AFTER: Duration = Duration::from_secs(90);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// 单帧上限 4 MiB（protocol.md「传输与外壳」）
const MAX_MESSAGE: usize = 4 << 20;

pub async fn ws_handler(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<PeerAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.max_message_size(MAX_MESSAGE)
        .max_frame_size(MAX_MESSAGE)
        .on_upgrade(move |socket| async move { run(state, socket, peer.0).await })
}

enum Auth {
    Ok(Box<Server>),
    Invalid,
    Deleted,
}

async fn authenticate(state: &AppState, token: &str) -> anyhow::Result<Auth> {
    let hash = secret::sha256_hex(token);
    if let Some(server) = db::servers::by_token_hash(&state.db, &hash).await? {
        return Ok(Auth::Ok(Box::new(server)));
    }
    if db::servers::is_deleted_token(&state.db, &hash).await? {
        return Ok(Auth::Deleted);
    }
    Ok(Auth::Invalid)
}

async fn run(state: AppState, socket: WebSocket, peer: SocketAddr) {
    let (mut sink, mut stream) = socket.split();

    // 第一条消息必须是 Hello
    let (hello_id, hello) =
        match tokio::time::timeout(HELLO_TIMEOUT, next_message(&mut stream)).await {
            Ok(Some(pb::AgentMessage {
                id,
                body: Some(AgentBody::Hello(hello)),
                ..
            })) => (id, hello),
            _ => {
                tracing::debug!(%peer, "没有收到 Hello，断开");
                return;
            }
        };
    let sync = hello.agent_version == VERSION;
    let reply = |status: Status, sync: bool| pb::MasterMessage {
        id: 1,
        reply_to: hello_id,
        body: Some(pb::master_message::Body::HelloResult(pb::HelloResult {
            status: status as i32,
            master_version: VERSION.to_string(),
            sync,
        })),
    };

    let server = match authenticate(&state, &hello.token).await {
        Ok(Auth::Ok(server)) => server,
        Ok(Auth::Deleted) => {
            tracing::info!(%peer, "已删除服务器的 Agent 连上来，通知它卸载");
            let _ = send(&mut sink, reply(Status::ServerDeleted, false)).await;
            return;
        }
        Ok(Auth::Invalid) => {
            tracing::warn!(%peer, token = %redact(&hello.token), "Agent 的 Token 无效");
            let _ = send(&mut sink, reply(Status::InvalidToken, false)).await;
            return;
        }
        Err(err) => {
            tracing::error!(%peer, "校验 Agent Token 出错: {err:#}");
            return;
        }
    };

    let arch = pb::Arch::try_from(hello.arch).unwrap_or(pb::Arch::Unspecified);
    let (outbox_tx, outbox_rx) = mpsc::channel(64);
    let Some(handle) = state.hub.register(
        server.id,
        sync,
        hello.agent_version.clone(),
        arch,
        outbox_tx,
    ) else {
        tracing::warn!(server_id = server.id, %peer, "这台服务器已经有在线的连接，拒绝重复连接");
        let _ = send(&mut sink, reply(Status::DuplicateConnection, false)).await;
        return;
    };

    if let Err(err) = on_hello(&state, &server, &hello, arch).await {
        tracing::error!(server_id = server.id, "记录 Hello 出错: {err:#}");
    }
    if send(&mut sink, reply(Status::Ok, sync)).await.is_err() {
        state.hub.unregister(server.id, handle.conn_id);
        return;
    }
    tracing::info!(
        server_id = server.id,
        name = %server.name,
        %peer,
        version = %hello.agent_version,
        sync,
        "Agent 已连接"
    );

    let mut writer = tokio::spawn(write_loop(sink, outbox_rx));

    // 版本一致时，状态版本和 Agent 已应用的不相等就推送（用不相等而不是「更小」，数据库从备份恢复后也能追平）
    if sync {
        match state.states.current(&state, server.id).await {
            Ok(Some(current)) if current.version != hello.state_version => {
                let _ = handle
                    .push(pb::master_message::Body::DesiredState((*current).clone()))
                    .await;
            }
            Ok(_) => {}
            Err(err) => tracing::error!(server_id = server.id, "生成期望状态出错: {err:#}"),
        }
    }

    let mut last_seen = Instant::now();
    let mut check = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            frame = stream.next() => {
                let Some(Ok(frame)) = frame else { break };
                last_seen = Instant::now();
                match frame {
                    Message::Binary(data) => match pb::AgentMessage::decode(data) {
                        Ok(msg) => on_message(&state, &handle, msg).await,
                        Err(err) => tracing::debug!(server_id = server.id, "解析 Agent 消息失败: {err}"),
                    },
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            _ = check.tick() => {
                if last_seen.elapsed() > OFFLINE_AFTER {
                    tracing::warn!(server_id = server.id, "90 秒没有收到 Agent 的消息，判定离线");
                    break;
                }
            }
            _ = handle.close.notified() => break,
            _ = &mut writer => break,
        }
    }

    state.hub.unregister(server.id, handle.conn_id);
    writer.abort();
    if let Err(err) = db::servers::touch(&state.db, server.id).await {
        tracing::warn!(server_id = server.id, "记录最后在线时间出错: {err}");
    }
    tracing::info!(server_id = server.id, name = %server.name, "Agent 已断开");
}

/// 记下 Hello 的内容；实例 ID 变了说明 Agent 重启过，计数器从头算。
async fn on_hello(
    state: &AppState,
    server: &Server,
    hello: &pb::Hello,
    arch: pb::Arch,
) -> anyhow::Result<()> {
    let arch = match arch {
        pb::Arch::Amd64 => "amd64",
        pb::Arch::Arm64 => "arm64",
        pb::Arch::Unspecified => "",
    };
    db::servers::record_hello(
        &state.db,
        server.id,
        &hello.agent_version,
        arch,
        &hello.rolled_back_from,
    )
    .await?;
    traffic::check_instance(&state.db, server.id, hello.instance_id).await?;
    Ok(())
}

async fn on_message(state: &AppState, handle: &AgentHandle, msg: pb::AgentMessage) {
    if msg.reply_to != 0 {
        if let Some(body) = msg.body {
            handle.resolve(msg.reply_to, body);
        }
        return;
    }
    match msg.body {
        Some(AgentBody::TrafficReport(report)) => {
            if let Err(err) = traffic::ingest(state, handle.server_id, report).await {
                tracing::error!(server_id = handle.server_id, "流量入账出错: {err:#}");
            }
        }
        Some(AgentBody::StateReport(report)) => {
            if let Err(err) = crate::state::record_report(state, handle.server_id, report).await {
                tracing::error!(server_id = handle.server_id, "记录状态上报出错: {err:#}");
            }
        }
        // 不认识的消息（对方版本更新）和不该出现的消息一律忽略
        _ => {}
    }
}

/// 读下一条 Agent 消息；连接关闭时返回 None。
async fn next_message(stream: &mut SplitStream<WebSocket>) -> Option<pb::AgentMessage> {
    while let Some(frame) = stream.next().await {
        match frame {
            Ok(Message::Binary(data)) => match pb::AgentMessage::decode(data) {
                Ok(msg) => return Some(msg),
                Err(err) => tracing::debug!("解析 Agent 消息失败: {err}"),
            },
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
    None
}

async fn send(sink: &mut SplitSink<WebSocket, Message>, msg: pb::MasterMessage) -> Result<(), ()> {
    let frame = Message::Binary(Bytes::from(msg.encode_to_vec()));
    match tokio::time::timeout(WRITE_TIMEOUT, sink.send(frame)).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(()),
    }
}

/// 写循环：消息和心跳都从这里发，写操作串行化；写失败或超时就结束，读循环随之断开。
async fn write_loop(
    mut sink: SplitSink<WebSocket, Message>,
    mut outbox: mpsc::Receiver<pb::MasterMessage>,
) {
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    loop {
        let frame = tokio::select! {
            msg = outbox.recv() => match msg {
                Some(msg) => Message::Binary(Bytes::from(msg.encode_to_vec())),
                None => break,
            },
            _ = ping.tick() => Message::Ping(Bytes::new()),
        };
        match tokio::time::timeout(WRITE_TIMEOUT, sink.send(frame)).await {
            Ok(Ok(())) => {}
            _ => break,
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), sink.close()).await;
}

/// 秘密只留前 4 个字符打日志（code-style.md「通用」）。
fn redact(secret: &str) -> String {
    let head: String = secret.chars().take(4).collect();
    format!("{head}****")
}
