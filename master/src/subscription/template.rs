//! 订阅模板：只维护一份 Mihomo 模板，全局生效；其他完整配置格式由系统翻译（subscription.md「规则模板」）。
//!
//! 节点怎么进代理组：用 Mihomo 自己的写法 `include-all-proxies: true`（可配 `filter`、`exclude-filter`），
//! 服务端把它展开成具体的节点名，所以各格式都不用支持 filter，提示节点也不会混进代理组。
//! 翻译只支持明确的子集（主控计划「实施方案」），其余的 Mihomo 原样保留，其他格式丢掉并写进兼容性报告。

use std::collections::{HashMap, HashSet};

use anyhow::{Context, anyhow, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlLoader};

use super::node::Format;
use crate::db::settings;

/// 内置模板：只用能完整翻译到所有格式的写法。
pub const BUILTIN: &str = r#"# open-proxy 内置模板：只用能完整翻译到所有客户端的写法。
# 节点由服务端插进 include-all-proxies 的代理组（可以配 filter、exclude-filter 按名字筛选）。
mixed-port: 7890
allow-lan: false
mode: rule
log-level: info
ipv6: false
proxy-groups:
  - name: 手动选择
    type: select
    proxies:
      - 自动选最快
      - DIRECT
    include-all-proxies: true
  - name: 自动选最快
    type: url-test
    url: http://www.gstatic.com/generate_204
    interval: 300
    tolerance: 50
    include-all-proxies: true
rules:
  - IP-CIDR,127.0.0.0/8,DIRECT,no-resolve
  - IP-CIDR,10.0.0.0/8,DIRECT,no-resolve
  - IP-CIDR,172.16.0.0/12,DIRECT,no-resolve
  - IP-CIDR,192.168.0.0/16,DIRECT,no-resolve
  - DOMAIN-SUFFIX,cn,DIRECT
  - GEOIP,CN,DIRECT
  - MATCH,手动选择
"#;

/// 设置里保存的模板（settings 表的 `template` 键）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateSettings {
    /// builtin / custom（粘贴或上传）/ remote（远程地址定时拉取）
    #[serde(default = "builtin_source")]
    pub source: String,
    /// custom 的正文，或 remote 最近一次拉取成功的正文
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default = "default_refresh_hours")]
    pub refresh_hours: u32,
    /// remote 最近一次拉取的时间（Unix 毫秒）
    #[serde(default)]
    pub last_fetch_at: Option<i64>,
    #[serde(default)]
    pub last_error: String,
}

fn builtin_source() -> String {
    "builtin".to_string()
}

/// 远程模板默认每 24 小时拉取一次（subscription.md「待讨论」在主控计划里定下）。
fn default_refresh_hours() -> u32 {
    24
}

impl Default for TemplateSettings {
    fn default() -> Self {
        Self {
            source: builtin_source(),
            content: String::new(),
            remote_url: None,
            refresh_hours: default_refresh_hours(),
            last_fetch_at: None,
            last_error: String::new(),
        }
    }
}

impl TemplateSettings {
    /// 当前生效的模板正文：远程模板还没拉取成功过时用内置的。
    pub fn text(&self) -> &str {
        match self.source.as_str() {
            "custom" | "remote" if !self.content.trim().is_empty() => &self.content,
            _ => BUILTIN,
        }
    }
}

pub async fn load(pool: &SqlitePool) -> anyhow::Result<TemplateSettings> {
    Ok(settings::get(pool, settings::TEMPLATE)
        .await?
        .unwrap_or_default())
}

pub async fn save(pool: &SqlitePool, t: &TemplateSettings) -> anyhow::Result<()> {
    settings::set(pool, settings::TEMPLATE, t).await
}

/// 拉取远程模板：20 秒超时，最大 1 MiB，要能解析成模板。
pub async fn fetch_remote(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("op-master")
        .build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .context("请求远程模板")?
        .error_for_status()
        .context("远程模板返回错误状态")?;
    let body = resp.bytes().await.context("读取远程模板")?;
    if body.len() > 1 << 20 {
        bail!("远程模板超过 1 MiB");
    }
    let text = String::from_utf8(body.to_vec()).context("远程模板不是 UTF-8 文本")?;
    parse(&text)?;
    Ok(text)
}

/// 解析后的模板。
pub struct Template {
    /// 模板的根（Mihomo 输出时在它上面改）
    pub root: Hash,
    pub groups: Vec<GroupDef>,
    pub rules: Vec<RuleDef>,
    pub providers: HashMap<String, Provider>,
}

pub struct GroupDef {
    pub name: String,
    pub kind: String,
    pub explicit: Vec<String>,
    pub include_all: bool,
    pub filter: Option<Regex>,
    pub exclude: Option<Regex>,
    pub url: Option<String>,
    pub interval: Option<i64>,
    pub tolerance: Option<i64>,
    /// 引用了 proxy-providers（其他格式翻译不了）
    pub uses_providers: bool,
}

pub enum RuleDef {
    /// DOMAIN、DOMAIN-SUFFIX、DOMAIN-KEYWORD、IP-CIDR、IP-CIDR6、GEOIP
    Simple {
        kind: String,
        value: String,
        policy: String,
        no_resolve: bool,
    },
    RuleSet {
        provider: String,
        policy: String,
    },
    Match {
        policy: String,
    },
    /// 子集之外的规则，原文
    Other(String),
}

pub struct Provider {
    pub behavior: String,
    pub format: String,
    pub url: Option<String>,
}

/// 展开后的代理组：成员都是具体的名字（节点、其他组、DIRECT、REJECT）。
pub struct Group {
    pub name: String,
    pub kind: String,
    pub members: Vec<String>,
    pub url: Option<String>,
    pub interval: Option<i64>,
    pub tolerance: Option<i64>,
}

pub const SIMPLE_RULES: [&str; 6] = [
    "DOMAIN",
    "DOMAIN-SUFFIX",
    "DOMAIN-KEYWORD",
    "IP-CIDR",
    "IP-CIDR6",
    "GEOIP",
];
pub const GROUP_KINDS: [&str; 4] = ["select", "url-test", "fallback", "load-balance"];
const BUILTIN_POLICIES: [&str; 2] = ["DIRECT", "REJECT"];

fn str_of(h: &Hash, key: &str) -> Option<String> {
    match h.get(&Yaml::String(key.to_string())) {
        Some(Yaml::String(s)) => Some(s.clone()),
        Some(Yaml::Integer(i)) => Some(i.to_string()),
        _ => None,
    }
}

fn int_of(h: &Hash, key: &str) -> Option<i64> {
    match h.get(&Yaml::String(key.to_string())) {
        Some(Yaml::Integer(i)) => Some(*i),
        Some(Yaml::String(s)) => s.parse().ok(),
        _ => None,
    }
}

fn bool_of(h: &Hash, key: &str) -> bool {
    matches!(
        h.get(&Yaml::String(key.to_string())),
        Some(Yaml::Boolean(true))
    )
}

/// Mihomo 的 filter 可以用反引号分隔多个正则，任意一个匹配就算。
fn compile_filter(s: &str) -> anyhow::Result<Regex> {
    let parts: Vec<String> = s
        .split('`')
        .filter(|p| !p.is_empty())
        .map(|p| format!("(?:{p})"))
        .collect();
    Regex::new(&parts.join("|")).map_err(|e| anyhow!("filter 不是合法的正则「{s}」: {e}"))
}

/// 解析模板正文。
pub fn parse(text: &str) -> anyhow::Result<Template> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| anyhow!("模板不是合法的 YAML: {e}"))?;
    let Some(Yaml::Hash(root)) = docs.into_iter().next() else {
        bail!("模板的顶层要是一个映射（Mihomo 配置）");
    };

    let mut groups = Vec::new();
    if let Some(Yaml::Array(list)) = root.get(&Yaml::String("proxy-groups".into())) {
        for item in list {
            let Yaml::Hash(g) = item else { continue };
            let name = str_of(g, "name").context("代理组缺少 name")?;
            let explicit = match g.get(&Yaml::String("proxies".into())) {
                Some(Yaml::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            };
            groups.push(GroupDef {
                kind: str_of(g, "type").unwrap_or_else(|| "select".to_string()),
                include_all: bool_of(g, "include-all-proxies") || bool_of(g, "include-all"),
                filter: str_of(g, "filter")
                    .map(|f| compile_filter(&f))
                    .transpose()?,
                exclude: str_of(g, "exclude-filter")
                    .map(|f| compile_filter(&f))
                    .transpose()?,
                url: str_of(g, "url"),
                interval: int_of(g, "interval"),
                tolerance: int_of(g, "tolerance"),
                uses_providers: g.contains_key(&Yaml::String("use".into())),
                explicit,
                name,
            });
        }
    }
    if groups.is_empty() {
        bail!("模板里没有 proxy-groups，节点没地方放");
    }

    let mut rules = Vec::new();
    if let Some(Yaml::Array(list)) = root.get(&Yaml::String("rules".into())) {
        for item in list {
            let Some(raw) = item.as_str() else { continue };
            rules.push(parse_rule(raw));
        }
    }

    let mut providers = HashMap::new();
    if let Some(Yaml::Hash(list)) = root.get(&Yaml::String("rule-providers".into())) {
        for (k, v) in list {
            let (Some(name), Yaml::Hash(p)) = (k.as_str(), v) else {
                continue;
            };
            providers.insert(
                name.to_string(),
                Provider {
                    behavior: str_of(p, "behavior").unwrap_or_default(),
                    format: str_of(p, "format").unwrap_or_else(|| "yaml".to_string()),
                    url: str_of(p, "url"),
                },
            );
        }
    }
    Ok(Template {
        root,
        groups,
        rules,
        providers,
    })
}

fn parse_rule(raw: &str) -> RuleDef {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    let no_resolve = parts.len() > 3 && parts[3..].contains(&"no-resolve");
    match parts.as_slice() {
        ["MATCH", policy, ..] => RuleDef::Match {
            policy: policy.to_string(),
        },
        ["RULE-SET", provider, policy, ..] => RuleDef::RuleSet {
            provider: provider.to_string(),
            policy: policy.to_string(),
        },
        [kind, value, policy, ..] if SIMPLE_RULES.contains(kind) => RuleDef::Simple {
            kind: kind.to_string(),
            value: value.to_string(),
            policy: policy.to_string(),
            no_resolve,
        },
        _ => RuleDef::Other(raw.to_string()),
    }
}

impl Template {
    /// 把代理组展开成具体的成员名。`nodes` 是这份订阅里的节点名（不含提示节点）。
    pub fn expand(&self, nodes: &[String]) -> Vec<Group> {
        let group_names: HashSet<&str> = self.groups.iter().map(|g| g.name.as_str()).collect();
        let node_names: HashSet<&str> = nodes.iter().map(String::as_str).collect();
        self.groups
            .iter()
            .map(|g| {
                let mut members: Vec<String> = g
                    .explicit
                    .iter()
                    .filter(|m| {
                        group_names.contains(m.as_str())
                            || BUILTIN_POLICIES.contains(&m.as_str())
                            || node_names.contains(m.as_str())
                    })
                    .cloned()
                    .collect();
                if g.include_all {
                    for n in nodes {
                        let included = g.filter.as_ref().is_none_or(|f| f.is_match(n));
                        let excluded = g.exclude.as_ref().is_some_and(|f| f.is_match(n));
                        if included && !excluded && !members.contains(n) {
                            members.push(n.clone());
                        }
                    }
                }
                if members.is_empty() {
                    members.push("DIRECT".to_string());
                }
                Group {
                    name: g.name.clone(),
                    kind: g.kind.clone(),
                    members,
                    url: g.url.clone(),
                    interval: g.interval,
                    tolerance: g.tolerance,
                }
            })
            .collect()
    }

    /// 规则的策略是不是翻译后还存在的名字（代理组或 DIRECT、REJECT）。
    pub fn known_policy(&self, policy: &str) -> bool {
        BUILTIN_POLICIES.contains(&policy) || self.groups.iter().any(|g| g.name == policy)
    }

    /// 某个 RULE-SET 能不能翻译到这个格式，不能时返回原因。
    pub fn rule_set_support(&self, format: Format, provider: &str) -> Result<&Provider, String> {
        let p = self
            .providers
            .get(provider)
            .ok_or_else(|| format!("rule-providers 里没有「{provider}」"))?;
        if p.url.is_none() {
            return Err("只支持远程（http）规则集".to_string());
        }
        match format {
            Format::Stash if p.format != "mrs" => Ok(p),
            Format::Stash => Err(".mrs 二进制规则集翻译不了".to_string()),
            Format::QuantumultX => Err("Quantumult X 的远程规则格式不同，暂不翻译".to_string()),
            _ if p.behavior == "classical" && p.format == "text" => Ok(p),
            _ => Err("只翻译 behavior: classical、format: text 的规则集".to_string()),
        }
    }

    /// 翻译到某个格式时丢掉了什么（兼容性报告）。Mihomo 原样使用，不丢东西。
    pub fn report(&self, format: Format) -> Vec<Dropped> {
        let mut dropped = Vec::new();
        if format == Format::Mihomo {
            return dropped;
        }
        if format == Format::V2ray {
            dropped.push(Dropped::new(
                "all",
                "代理组和规则",
                "分享链接只有节点，没有代理组和规则",
            ));
            return dropped;
        }
        for key in ["dns", "sniffer", "tun", "proxy-providers", "hosts"] {
            if self.root.contains_key(&Yaml::String(key.into())) {
                dropped.push(Dropped::new(
                    "section",
                    key,
                    "这一段不翻译，客户端用自己的设置",
                ));
            }
        }
        for g in &self.groups {
            if !GROUP_KINDS.contains(&g.kind.as_str()) {
                dropped.push(Dropped::new(
                    "proxy_group",
                    &g.name,
                    &format!("{} 类型的代理组翻译成 select", g.kind),
                ));
            }
            if g.uses_providers {
                dropped.push(Dropped::new(
                    "proxy_group",
                    &g.name,
                    "use 引用的 proxy-providers 翻译不了",
                ));
            }
        }
        for rule in &self.rules {
            match rule {
                RuleDef::Other(raw) => {
                    dropped.push(Dropped::new("rule", raw, "这种规则不在翻译子集里"))
                }
                RuleDef::RuleSet {
                    provider, policy, ..
                } => {
                    if let Err(reason) = self.rule_set_support(format, provider) {
                        dropped.push(Dropped::new(
                            "rule",
                            &format!("RULE-SET,{provider},{policy}"),
                            &reason,
                        ));
                    }
                }
                _ => {}
            }
        }
        dropped
    }
}

/// 兼容性报告里的一项。
#[derive(Serialize)]
pub struct Dropped {
    pub kind: &'static str,
    pub item: String,
    pub reason: String,
}

impl Dropped {
    fn new(kind: &'static str, item: &str, reason: &str) -> Self {
        Self {
            kind,
            item: item.to_string(),
            reason: reason.to_string(),
        }
    }
}
