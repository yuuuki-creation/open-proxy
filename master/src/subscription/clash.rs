//! Clash 系的 YAML：Mihomo（模板原样 + 节点，模板里的其他写法都保留）、Stash、Shadowrocket（翻译子集）。
//! 各协议的字段写法见参考笔记 reference/mmwx/code-master-subscription.md「各格式关键写法」。

use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter};

use super::node::{Format, Proto, ProxyNode, supports};
use super::template::{GROUP_KINDS, Group, RuleDef, Template};

fn s(v: &str) -> Yaml {
    Yaml::String(v.to_string())
}

fn put(h: &mut Hash, key: &str, value: Yaml) {
    h.insert(s(key), value);
}

fn emit(root: Hash) -> String {
    let mut out = String::new();
    let mut emitter = YamlEmitter::new(&mut out);
    if emitter.dump(&Yaml::Hash(root)).is_err() {
        return String::new();
    }
    let body = out.strip_prefix("---\n").unwrap_or(&out);
    format!("{body}\n")
}

/// 一个节点的 YAML 写法。
fn proxy(node: &ProxyNode, format: Format) -> Yaml {
    let mut h = Hash::new();
    put(&mut h, "name", s(&node.name));
    let server = s(&node.server);
    let port = Yaml::Integer(i64::from(node.port));
    match &node.proto {
        Proto::Vless {
            uuid,
            public_key,
            short_id,
            sni,
        } => {
            put(&mut h, "type", s("vless"));
            put(&mut h, "server", server);
            put(&mut h, "port", port);
            put(&mut h, "uuid", s(uuid));
            put(&mut h, "network", s("tcp"));
            put(&mut h, "tls", Yaml::Boolean(true));
            put(&mut h, "udp", Yaml::Boolean(true));
            put(&mut h, "flow", s("xtls-rprx-vision"));
            put(&mut h, "servername", s(sni));
            put(&mut h, "client-fingerprint", s("chrome"));
            let mut reality = Hash::new();
            put(&mut reality, "public-key", s(public_key));
            put(&mut reality, "short-id", s(short_id));
            put(&mut h, "reality-opts", Yaml::Hash(reality));
        }
        Proto::Hysteria2 {
            password,
            sni,
            obfs_password,
            ports,
            cert_sha256,
        } => {
            put(&mut h, "type", s("hysteria2"));
            put(&mut h, "server", server);
            put(&mut h, "port", port);
            // Stash 的 Hysteria2 密码字段叫 auth
            let password_key = if format == Format::Stash {
                "auth"
            } else {
                "password"
            };
            put(&mut h, password_key, s(password));
            if let (Some((start, end)), Format::Mihomo) = (ports, format) {
                put(&mut h, "ports", s(&format!("{start}-{end}")));
            }
            if let Some(sni) = sni {
                put(&mut h, "sni", s(sni));
            }
            if let Some(obfs) = obfs_password {
                put(&mut h, "obfs", s("salamander"));
                put(&mut h, "obfs-password", s(obfs));
            }
            put_pin(&mut h, format, cert_sha256.as_deref());
        }
        Proto::AnyTls {
            password,
            sni,
            cert_sha256,
        } => {
            put(&mut h, "type", s("anytls"));
            put(&mut h, "server", server);
            put(&mut h, "port", port);
            put(&mut h, "password", s(password));
            put(&mut h, "udp", Yaml::Boolean(true));
            put(&mut h, "client-fingerprint", s("chrome"));
            if let Some(sni) = sni {
                put(&mut h, "sni", s(sni));
            }
            put_pin(&mut h, format, cert_sha256.as_deref());
        }
        Proto::Ss { method, password } => {
            put(&mut h, "type", s("ss"));
            put(&mut h, "server", server);
            put(&mut h, "port", port);
            put(&mut h, "cipher", s(method));
            put(&mut h, "password", s(password));
            put(&mut h, "udp", Yaml::Boolean(true));
        }
        Proto::Mieru { username, password } => {
            put(&mut h, "type", s("mieru"));
            put(&mut h, "server", server);
            put(&mut h, "port", port);
            put(&mut h, "transport", s("TCP"));
            put(&mut h, "username", s(username));
            put(&mut h, "password", s(password));
        }
    }
    Yaml::Hash(h)
}

/// 自签证书：跳过 CA 校验，改为固定证书指纹。
fn put_pin(h: &mut Hash, format: Format, cert_sha256: Option<&str>) {
    if let Some(fp) = cert_sha256 {
        put(h, "skip-cert-verify", Yaml::Boolean(true));
        let key = if format == Format::Stash {
            "server-cert-fingerprint"
        } else {
            "fingerprint"
        };
        put(h, key, s(fp));
    }
}

fn group(g: &Group, format: Format) -> Yaml {
    let mut h = Hash::new();
    let kind = if GROUP_KINDS.contains(&g.kind.as_str()) {
        g.kind.as_str()
    } else {
        "select"
    };
    put(&mut h, "name", s(&g.name));
    put(&mut h, "type", s(kind));
    put(
        &mut h,
        "proxies",
        Yaml::Array(g.members.iter().map(|m| s(m)).collect()),
    );
    if kind != "select" {
        let url = g
            .url
            .clone()
            .unwrap_or_else(|| "http://www.gstatic.com/generate_204".to_string());
        // Stash 用 benchmark-url
        let url_key = if format == Format::Stash {
            "benchmark-url"
        } else {
            "url"
        };
        put(&mut h, url_key, s(&url));
        put(&mut h, "interval", Yaml::Integer(g.interval.unwrap_or(300)));
        if kind == "url-test"
            && let Some(t) = g.tolerance
        {
            put(&mut h, "tolerance", Yaml::Integer(t));
        }
    }
    Yaml::Hash(h)
}

/// Mihomo：在模板上替换 proxies、展开代理组，其他内容原样保留。
pub fn mihomo(tpl: &Template, hints: &[ProxyNode], nodes: &[ProxyNode]) -> String {
    let format = Format::Mihomo;
    let usable: Vec<&ProxyNode> = nodes
        .iter()
        .filter(|n| supports(format, &n.proto))
        .collect();
    let names: Vec<String> = usable.iter().map(|n| n.name.clone()).collect();
    let groups = tpl.expand(&names);

    let mut root = tpl.root.clone();
    let proxies: Vec<Yaml> = hints
        .iter()
        .chain(usable.iter().copied())
        .map(|n| proxy(n, format))
        .collect();
    put(&mut root, "proxies", Yaml::Array(proxies));

    let key = s("proxy-groups");
    if let Some(Yaml::Array(orig)) = root.get(&key).cloned() {
        let mut rewritten = Vec::new();
        for item in orig {
            let Yaml::Hash(mut g) = item else { continue };
            let name = g
                .get(&s("name"))
                .and_then(Yaml::as_str)
                .unwrap_or("")
                .to_string();
            let Some(expanded) = groups.iter().find(|x| x.name == name) else {
                continue;
            };
            for k in [
                "include-all",
                "include-all-proxies",
                "filter",
                "exclude-filter",
            ] {
                g.remove(&s(k));
            }
            put(
                &mut g,
                "proxies",
                Yaml::Array(expanded.members.iter().map(|m| s(m)).collect()),
            );
            rewritten.push(Yaml::Hash(g));
        }
        put(&mut root, "proxy-groups", Yaml::Array(rewritten));
    }
    emit(root)
}

/// Stash、Shadowrocket：按翻译子集重新生成。
pub fn translated(
    tpl: &Template,
    hints: &[ProxyNode],
    nodes: &[ProxyNode],
    format: Format,
) -> String {
    let usable: Vec<&ProxyNode> = nodes
        .iter()
        .filter(|n| supports(format, &n.proto))
        .collect();
    let names: Vec<String> = usable.iter().map(|n| n.name.clone()).collect();
    let groups = tpl.expand(&names);

    let mut root = Hash::new();
    // 通用设置（标量）照搬
    for key in [
        "mixed-port",
        "port",
        "socks-port",
        "allow-lan",
        "mode",
        "log-level",
        "ipv6",
    ] {
        if let Some(v) = tpl.root.get(&s(key)) {
            root.insert(s(key), v.clone());
        }
    }
    let proxies: Vec<Yaml> = hints
        .iter()
        .chain(usable.iter().copied())
        .map(|n| proxy(n, format))
        .collect();
    put(&mut root, "proxies", Yaml::Array(proxies));
    put(
        &mut root,
        "proxy-groups",
        Yaml::Array(groups.iter().map(|g| group(g, format)).collect()),
    );

    let mut rules = Vec::new();
    let mut providers = Hash::new();
    let orig_providers = match tpl.root.get(&s("rule-providers")) {
        Some(Yaml::Hash(p)) => Some(p),
        _ => None,
    };
    for rule in &tpl.rules {
        match rule {
            RuleDef::Simple {
                kind,
                value,
                policy,
                no_resolve,
            } if tpl.known_policy(policy) => {
                let suffix = if *no_resolve { ",no-resolve" } else { "" };
                rules.push(s(&format!("{kind},{value},{policy}{suffix}")));
            }
            RuleDef::RuleSet {
                provider, policy, ..
            } if tpl.known_policy(policy) && tpl.rule_set_support(format, provider).is_ok() => {
                if let Some(p) = orig_providers.and_then(|p| p.get(&s(provider))) {
                    providers.insert(s(provider), p.clone());
                    rules.push(s(&format!("RULE-SET,{provider},{policy}")));
                }
            }
            RuleDef::Match { policy } if tpl.known_policy(policy) => {
                rules.push(s(&format!("MATCH,{policy}")));
            }
            _ => {}
        }
    }
    if !providers.is_empty() {
        put(&mut root, "rule-providers", Yaml::Hash(providers));
    }
    put(&mut root, "rules", Yaml::Array(rules));
    emit(root)
}
