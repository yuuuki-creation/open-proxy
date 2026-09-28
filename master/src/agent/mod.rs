//! Agent 网关：WebSocket 连接、Hello 认证、连接表、请求和回复。协议见 main 分支 protocol.md。
//! 每台服务器同时只允许一个连接（重复连接直接拒绝，不互相踢）；每个连接的写操作串行化并设超时。

mod session;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::pb::agentv1 as pb;

pub use session::ws_handler;

/// 在线的 Agent 连接表，外加每台服务器的网卡计数（算实时网速用，只放内存）。
#[derive(Default)]
pub struct Hub {
    conns: Mutex<HashMap<i64, Arc<AgentHandle>>>,
    nic: Mutex<HashMap<i64, NicSample>>,
    next_conn_id: AtomicU64,
    /// 最近一次升级失败的原因（成功或没升级过时没有）
    upgrade_errors: Mutex<HashMap<i64, String>>,
    /// REALITY 目标扫描的状态和结果，只放内存
    scans: Mutex<HashMap<i64, ScanState>>,
}

/// 一台服务器的 REALITY 目标扫描。
#[derive(Clone, Serialize)]
pub struct ScanState {
    /// idle / running / done / failed
    pub status: &'static str,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error: String,
    pub candidates: Vec<ScanCandidate>,
}

#[derive(Clone, Serialize)]
pub struct ScanCandidate {
    pub ip: String,
    pub domain: String,
    pub issuer: String,
    pub latency_ms: u32,
}

impl ScanState {
    fn idle() -> Self {
        Self {
            status: "idle",
            started_at: None,
            finished_at: None,
            error: String::new(),
            candidates: Vec::new(),
        }
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 一个已认证的 Agent 连接。
pub struct AgentHandle {
    pub conn_id: u64,
    pub server_id: i64,
    /// 版本一致，同步期望状态
    pub sync: bool,
    pub version: String,
    pub arch: pb::Arch,
    outbox: mpsc::Sender<pb::MasterMessage>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<pb::agent_message::Body>>>,
    close: Notify,
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[allow(dead_code)] // P6 用
    #[error("Agent 不在线")]
    Offline,
    #[error("等 Agent 回复超时")]
    Timeout,
    #[error("连接断开")]
    Disconnected,
}

impl AgentHandle {
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// 发推送（不需要回复）。发送队列满或连接已断时返回错误。
    pub async fn push(&self, body: pb::master_message::Body) -> Result<(), RequestError> {
        let msg = pb::MasterMessage {
            id: self.next_id(),
            reply_to: 0,
            body: Some(body),
        };
        self.outbox
            .send_timeout(msg, Duration::from_secs(10))
            .await
            .map_err(|_| RequestError::Disconnected)
    }

    /// 发请求并等回复，超时后到的回复直接丢弃（protocol.md「传输与外壳」）。
    pub async fn request(
        &self,
        body: pb::master_message::Body,
        timeout: Duration,
    ) -> Result<pb::agent_message::Body, RequestError> {
        let id = self.next_id();
        let (tx, rx) = oneshot::channel();
        self.lock_pending().insert(id, tx);
        let msg = pb::MasterMessage {
            id,
            reply_to: 0,
            body: Some(body),
        };
        if self
            .outbox
            .send_timeout(msg, Duration::from_secs(10))
            .await
            .is_err()
        {
            self.lock_pending().remove(&id);
            return Err(RequestError::Disconnected);
        }
        let result = tokio::time::timeout(timeout, rx).await;
        self.lock_pending().remove(&id);
        match result {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(_)) => Err(RequestError::Disconnected),
            Err(_) => Err(RequestError::Timeout),
        }
    }

    /// 收到回复：交给等它的请求；没人等（已经超时）就丢弃。
    fn resolve(&self, reply_to: u64, body: pb::agent_message::Body) {
        if let Some(tx) = self.lock_pending().remove(&reply_to) {
            let _ = tx.send(body);
        }
    }

    /// 让会话断开。
    pub fn close(&self) {
        self.close.notify_one();
    }

    fn lock_pending(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<pb::agent_message::Body>>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 网卡计数的上一次采样，用相邻两次的差算网速。
#[derive(Clone, Copy)]
struct NicSample {
    rx: u64,
    tx: u64,
    at: Instant,
    rx_speed: u64,
    tx_speed: u64,
}

impl Hub {
    fn lock_conns(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Arc<AgentHandle>>> {
        self.conns.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, server_id: i64) -> Option<Arc<AgentHandle>> {
        self.lock_conns().get(&server_id).cloned()
    }

    pub fn is_online(&self, server_id: i64) -> bool {
        self.lock_conns().contains_key(&server_id)
    }

    /// 登记新连接；这台服务器已经有连接时返回 None（重复连接）。
    fn register(
        &self,
        server_id: i64,
        sync: bool,
        version: String,
        arch: pb::Arch,
        outbox: mpsc::Sender<pb::MasterMessage>,
    ) -> Option<Arc<AgentHandle>> {
        let mut conns = self.lock_conns();
        if conns.contains_key(&server_id) {
            return None;
        }
        let handle = Arc::new(AgentHandle {
            conn_id: self.next_conn_id.fetch_add(1, Ordering::Relaxed) + 1,
            server_id,
            sync,
            version,
            arch,
            outbox,
            next_id: AtomicU64::new(2), // 1 已经给 HelloResult 用了
            pending: Mutex::new(HashMap::new()),
            close: Notify::new(),
        });
        conns.insert(server_id, handle.clone());
        Some(handle)
    }

    /// 连接断开时注销。只注销自己这条：确认连接表里登记的还是这条连接（architecture.md「要避免的坑」）。
    fn unregister(&self, server_id: i64, conn_id: u64) {
        let mut conns = self.lock_conns();
        if conns.get(&server_id).is_some_and(|h| h.conn_id == conn_id) {
            conns.remove(&server_id);
        }
    }

    /// 断开这台服务器的连接（换 Token、删除服务器时）。
    pub fn disconnect(&self, server_id: i64) {
        if let Some(handle) = self.get(server_id) {
            handle.close();
        }
    }

    /// 记一次网卡计数，更新网速（字节/秒）。`reset` 表示服务器重启过或换了网卡，计数不连续。
    pub fn record_nic(&self, server_id: i64, rx: u64, tx: u64, reset: bool) {
        let mut nic = self.nic.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let sample = match nic.get(&server_id) {
            Some(prev) if !reset && rx >= prev.rx && tx >= prev.tx => {
                let secs = now.duration_since(prev.at).as_secs_f64().max(1.0);
                NicSample {
                    rx,
                    tx,
                    at: now,
                    rx_speed: ((rx - prev.rx) as f64 / secs) as u64,
                    tx_speed: ((tx - prev.tx) as f64 / secs) as u64,
                }
            }
            _ => NicSample {
                rx,
                tx,
                at: now,
                rx_speed: 0,
                tx_speed: 0,
            },
        };
        nic.insert(server_id, sample);
    }

    pub fn set_upgrade_error(&self, server_id: i64, error: Option<String>) {
        let mut map = self
            .upgrade_errors
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match error {
            Some(e) => map.insert(server_id, e),
            None => map.remove(&server_id),
        };
    }

    pub fn upgrade_error(&self, server_id: i64) -> Option<String> {
        let map = self
            .upgrade_errors
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        map.get(&server_id).cloned()
    }

    /// 开始扫描；这台服务器已经在扫时返回 false。
    pub fn begin_scan(&self, server_id: i64) -> bool {
        let mut scans = self.scans.lock().unwrap_or_else(|e| e.into_inner());
        if scans.get(&server_id).is_some_and(|s| s.status == "running") {
            return false;
        }
        let mut state = ScanState::idle();
        state.status = "running";
        state.started_at = Some(now_rfc3339());
        scans.insert(server_id, state);
        true
    }

    pub fn finish_scan(
        &self,
        server_id: i64,
        outcome: Result<Vec<pb::RealityScanCandidate>, String>,
    ) {
        let mut scans = self.scans.lock().unwrap_or_else(|e| e.into_inner());
        let state = scans.entry(server_id).or_insert_with(ScanState::idle);
        state.finished_at = Some(now_rfc3339());
        match outcome {
            Ok(list) => {
                state.status = "done";
                state.candidates = list
                    .into_iter()
                    .map(|c| ScanCandidate {
                        ip: c.ip,
                        domain: c.domain,
                        issuer: c.issuer,
                        latency_ms: c.latency_ms,
                    })
                    .collect();
            }
            Err(e) => {
                state.status = "failed";
                state.error = e;
            }
        }
    }

    pub fn scan(&self, server_id: i64) -> ScanState {
        let scans = self.scans.lock().unwrap_or_else(|e| e.into_inner());
        scans
            .get(&server_id)
            .cloned()
            .unwrap_or_else(ScanState::idle)
    }

    /// 实时网速（接收、发送，字节/秒）；超过 30 秒没有新数据时算 0。
    pub fn speed(&self, server_id: i64) -> (u64, u64) {
        let nic = self.nic.lock().unwrap_or_else(|e| e.into_inner());
        match nic.get(&server_id) {
            Some(s) if s.at.elapsed() < Duration::from_secs(30) => (s.rx_speed, s.tx_speed),
            _ => (0, 0),
        }
    }
}
