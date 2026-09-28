//! 订阅里的节点：从数据库的节点、服务器、证书和用户凭据组装，再由各格式输出。
//! 「客户端 × 协议」能力表也在这里（subscription.md「节点与协议」）。

use std::collections::{HashMap, HashSet};

use sqlx::SqlitePool;

use crate::db::{self, nodes::Node, users::User};

/// 输出格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Mihomo,
    Stash,
    Shadowrocket,
    Surge,
    Loon,
    QuantumultX,
    /// v2rayN、v2rayNG：base64 的分享链接
    V2ray,
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Mihomo,
        Format::Stash,
        Format::Shadowrocket,
        Format::Surge,
        Format::Loon,
        Format::QuantumultX,
        Format::V2ray,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Format::Mihomo => "mihomo",
            Format::Stash => "stash",
            Format::Shadowrocket => "shadowrocket",
            Format::Surge => "surge",
            Format::Loon => "loon",
            Format::QuantumultX => "quantumultx",
            Format::V2ray => "v2ray",
        }
    }

    /// 解析 `?format=` 参数，兼容几个常见的别名。
    pub fn parse(s: &str) -> Option<Format> {
        Some(match s.to_ascii_lowercase().as_str() {
            "mihomo" | "clash" | "clashmeta" | "clash.meta" | "meta" => Format::Mihomo,
            "stash" => Format::Stash,
            "shadowrocket" => Format::Shadowrocket,
            "surge" => Format::Surge,
            "loon" => Format::Loon,
            "quantumultx" | "quantumult-x" | "qx" => Format::QuantumultX,
            "v2ray" | "v2rayn" | "v2rayng" | "base64" | "links" => Format::V2ray,
            _ => return None,
        })
    }

    /// 按 User-Agent 识别格式。顺序要注意：Stash 的 UA 带 Clash 字样，要排在 Clash 前面。
    pub fn detect(user_agent: &str) -> Format {
        let ua = user_agent.to_ascii_lowercase();
        let has = |s: &str| ua.contains(s);
        if has("stash") {
            Format::Stash
        } else if has("shadowrocket") {
            Format::Shadowrocket
        } else if has("quantumult%20x") || has("quantumult x") || has("quantumultx") {
            Format::QuantumultX
        } else if has("surge") {
            Format::Surge
        } else if has("loon") {
            Format::Loon
        } else if has("v2rayn")
            || has("v2rayng")
            || has("v2box")
            || has("nekobox")
            || has("nekoray")
        {
            Format::V2ray
        } else {
            // Clash Verge Rev、FlClash、Mihomo Party 等，以及浏览器和不认识的客户端
            Format::Mihomo
        }
    }
}

/// 订阅里的一个节点。
#[derive(Clone, Debug)]
pub struct ProxyNode {
    pub name: String,
    pub server: String,
    pub port: u16,
    pub proto: Proto,
}

#[derive(Clone, Debug)]
pub enum Proto {
    Vless {
        uuid: String,
        public_key: String,
        short_id: String,
        /// 伪装目标，同时是 SNI
        sni: String,
    },
    Hysteria2 {
        password: String,
        sni: Option<String>,
        obfs_password: Option<String>,
        /// 端口跳跃范围
        ports: Option<(u16, u16)>,
        /// 自签证书的 SHA-256（小写十六进制），客户端固定指纹
        cert_sha256: Option<String>,
    },
    AnyTls {
        password: String,
        sni: Option<String>,
        cert_sha256: Option<String>,
    },
    Ss {
        method: String,
        /// Shadowsocks 2022 是「服务端主密钥:用户密钥」
        password: String,
    },
    Mieru {
        username: String,
        password: String,
    },
}

/// 这个格式能不能输出这个节点（subscription.md「客户端 × 协议」；Shadowrocket 一列按官方说明定，待实测）。
pub fn supports(format: Format, proto: &Proto) -> bool {
    match proto {
        Proto::Vless { .. } => format != Format::Surge,
        Proto::Hysteria2 { obfs_password, .. } => match format {
            Format::QuantumultX => false,
            // Surge 不支持 salamander 混淆
            Format::Surge => obfs_password.is_none(),
            _ => true,
        },
        Proto::AnyTls { .. } | Proto::Ss { .. } => true,
        Proto::Mieru { .. } => matches!(format, Format::Mihomo | Format::V2ray),
    }
}

/// 节点在哪些格式里看不到（面板上提示）。只看协议和混淆，不需要凭据。
pub fn hidden_in(node: &Node) -> Vec<&'static str> {
    let params = node.params();
    let proto = match node.protocol.as_str() {
        "vless_reality" => Proto::Vless {
            uuid: String::new(),
            public_key: String::new(),
            short_id: String::new(),
            sni: String::new(),
        },
        "hysteria2" => Proto::Hysteria2 {
            password: String::new(),
            sni: None,
            obfs_password: (!params.obfs_password.is_empty()).then_some(params.obfs_password),
            ports: None,
            cert_sha256: None,
        },
        "anytls" => Proto::AnyTls {
            password: String::new(),
            sni: None,
            cert_sha256: None,
        },
        "shadowsocks2022" => Proto::Ss {
            method: String::new(),
            password: String::new(),
        },
        _ => Proto::Mieru {
            username: String::new(),
            password: String::new(),
        },
    };
    Format::ALL
        .into_iter()
        .filter(|f| !supports(*f, &proto))
        .map(Format::name)
        .collect()
}

/// 这个用户能用的节点，按订阅顺序。凭据查不到的节点跳过，不回退到任何默认凭据。
pub async fn user_nodes(pool: &SqlitePool, user: &User) -> anyhow::Result<Vec<ProxyNode>> {
    let plan_nodes = db::plans::node_ids(pool).await?;
    let allowed: HashSet<i64> = plan_nodes
        .get(&user.plan_id)
        .map(|ids| ids.iter().copied().collect())
        .unwrap_or_default();
    let servers: HashMap<i64, db::servers::Server> = db::servers::list(pool)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let mut certs: HashMap<i64, Option<db::servers::Certificate>> = HashMap::new();
    let mut result = Vec::new();
    let mut used_names = HashSet::new();
    for node in db::nodes::list(pool).await? {
        if !node.enabled || !allowed.contains(&node.id) {
            continue;
        }
        let Some(server) = servers.get(&node.server_id) else {
            continue;
        };
        let cert = match certs.get(&server.id) {
            Some(cert) => cert.clone(),
            None => {
                let cert = db::servers::certificate(pool, Some(server.id)).await?;
                certs.insert(server.id, cert.clone());
                cert
            }
        };
        if let Some(proto) = proto_of(&node, server, cert.as_ref(), user) {
            let name = unique_name(&node.name, &mut used_names);
            result.push(ProxyNode {
                name,
                server: node
                    .address
                    .clone()
                    .unwrap_or_else(|| server.address.clone()),
                port: node.port as u16,
                proto,
            });
        }
    }
    Ok(result)
}

fn proto_of(
    node: &Node,
    server: &db::servers::Server,
    cert: Option<&db::servers::Certificate>,
    user: &User,
) -> Option<Proto> {
    let params = node.params();
    // 自动申请的证书用域名做 SNI、正常校验；自签证书固定指纹
    let (sni, cert_sha256) = match (server.cert_mode.as_str(), cert) {
        ("acme", _) => (server.cert_domain.clone(), None),
        (_, Some(c)) => (None, Some(c.sha256.clone())),
        _ => (None, None),
    };
    let proto = match node.protocol.as_str() {
        "vless_reality" => {
            if user.uuid.is_empty() || params.public_key.is_empty() {
                return None;
            }
            Proto::Vless {
                uuid: user.uuid.clone(),
                public_key: params.public_key,
                short_id: params.short_ids.first().cloned().unwrap_or_default(),
                sni: params.target_host,
            }
        }
        "hysteria2" => Proto::Hysteria2 {
            password: nonempty(&user.password)?,
            sni,
            obfs_password: (!params.obfs_password.is_empty()).then_some(params.obfs_password),
            ports: match (node.hop_port_start, node.hop_port_end) {
                (Some(s), Some(e)) => Some((s as u16, e as u16)),
                _ => None,
            },
            cert_sha256,
        },
        "anytls" => Proto::AnyTls {
            password: nonempty(&user.password)?,
            sni,
            cert_sha256,
        },
        "shadowsocks2022" => {
            if params.server_key.is_empty() || user.ss_key.is_empty() {
                return None;
            }
            Proto::Ss {
                method: params.method,
                password: format!("{}:{}", params.server_key, user.ss_key),
            }
        }
        "mieru" => Proto::Mieru {
            // Agent 里 Mieru 的用户名是用户 ID
            username: user.id.to_string(),
            password: nonempty(&user.password)?,
        },
        _ => return None,
    };
    Some(proto)
}

fn nonempty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// 节点名去掉 `=` 和 `,`（Surge、Loon 行格式的分隔符），重名时加序号。
fn unique_name(name: &str, used: &mut HashSet<String>) -> String {
    let clean: String = name
        .chars()
        .filter(|c| *c != '=' && *c != ',')
        .collect::<String>()
        .trim()
        .to_string();
    let base = if clean.is_empty() {
        "节点".to_string()
    } else {
        clean
    };
    let mut candidate = base.clone();
    let mut i = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base} {i}");
        i += 1;
    }
    candidate
}

/// 提示节点：用 aes-128-gcm 的 Shadowsocks 占位，所有客户端都能显示，但连不上任何东西。
pub fn hint_node(name: &str) -> ProxyNode {
    ProxyNode {
        name: name.chars().filter(|c| *c != '=' && *c != ',').collect(),
        server: "127.0.0.1".to_string(),
        port: 1,
        proto: Proto::Ss {
            method: "aes-128-gcm".to_string(),
            password: "open-proxy".to_string(),
        },
    }
}
