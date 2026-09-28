//! 主控托管的 Agent 二进制（architecture.md「发布、安装与升级」）：安装和升级都从主控下载。
//!
//! 发布时 Agent 二进制和签名放进 `master/agent-dist/` 嵌入主控（单文件部署）；
//! 也可以用 `--agent-dir` 指定目录（测试、Docker）。文件名：
//! `op-agent-linux-<arch>`（二进制）、`op-agent-linux-<arch>.sig`（对二进制全部内容的 Ed25519 签名，64 字节原始数据）。
//! 只托管和主控同版本的 Agent。

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use bytes::Bytes;
use rust_embed::RustEmbed;
use sha2::{Digest, Sha256};

#[derive(RustEmbed)]
#[folder = "agent-dist/"]
#[allow_missing = true]
struct Embedded;

/// 一个架构的 Agent 二进制。
#[derive(Clone)]
pub struct Binary {
    pub data: Bytes,
    pub sha256: [u8; 32],
    /// 没有签名时为 None（这样的二进制可以安装，但 Agent 拒绝用它升级）
    pub signature: Option<Vec<u8>>,
}

pub struct AgentDist {
    dir: Option<PathBuf>,
    cache: Mutex<HashMap<String, Option<Binary>>>,
}

pub const ARCHES: [&str; 2] = ["amd64", "arm64"];

impl AgentDist {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 取某个架构的二进制；第一次取时读文件、算哈希，之后用缓存。
    pub fn binary(&self, arch: &str) -> Option<Binary> {
        if !ARCHES.contains(&arch) {
            return None;
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(arch) {
            return entry.clone();
        }
        let loaded = self.load(arch);
        if loaded.is_none() {
            tracing::warn!(arch, "没有这个架构的 Agent 二进制");
        }
        cache.insert(arch.to_string(), loaded.clone());
        loaded
    }

    fn load(&self, arch: &str) -> Option<Binary> {
        let name = format!("op-agent-linux-{arch}");
        let (data, signature) = match &self.dir {
            Some(dir) => (
                Bytes::from(std::fs::read(dir.join(&name)).ok()?),
                std::fs::read(dir.join(format!("{name}.sig"))).ok(),
            ),
            // release 编译时嵌入的数据在二进制的只读段里，直接引用，不复制一份到堆上（每个架构几十 MB）
            None => (
                match Embedded::get(&name)?.data {
                    Cow::Borrowed(data) => Bytes::from_static(data),
                    Cow::Owned(data) => Bytes::from(data),
                },
                Embedded::get(&format!("{name}.sig")).map(|f| f.data.into_owned()),
            ),
        };
        let signature = signature.filter(|s| {
            let ok = s.len() == 64;
            if !ok {
                tracing::warn!(arch, "Agent 签名文件长度不是 64 字节，忽略");
            }
            ok
        });
        Some(Binary {
            sha256: Sha256::digest(&data).into(),
            data,
            signature,
        })
    }
}
