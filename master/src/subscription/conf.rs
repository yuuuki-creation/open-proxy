//! 行格式的完整配置：Surge、Loon、Quantumult X（按翻译子集生成）。
//! 各协议的写法见参考笔记 reference/mmwx/code-master-subscription.md「各格式关键写法」。

use std::fmt::Write as _;

use super::node::{Format, Proto, ProxyNode, supports};
use super::template::{GROUP_KINDS, Group, RuleDef, Template};

const TEST_URL: &str = "http://www.gstatic.com/generate_204";

/// Surge、Loon 的一个节点；不支持时返回 None（能力表已经过滤过，这里兜底）。
fn surge_line(node: &ProxyNode, format: Format) -> Option<String> {
    let (name, host, port) = (&node.name, &node.server, node.port);
    let pin = |fp: &Option<String>, key: &str| match fp {
        Some(fp) => format!(", skip-cert-verify=true, {key}={fp}"),
        None => String::new(),
    };
    let loon = format == Format::Loon;
    Some(match &node.proto {
        Proto::Vless {
            uuid,
            public_key,
            short_id,
            sni,
        } if loon => format!(
            "{name} = vless,{host},{port},\"{uuid}\",transport=tcp,over-tls=true,flow=xtls-rprx-vision,sni={sni},public-key=\"{public_key}\",short-id={short_id},udp=true"
        ),
        Proto::Hysteria2 {
            password,
            sni,
            obfs_password,
            ports,
            cert_sha256,
        } => {
            if loon {
                let mut line = format!("{name} = Hysteria2,{host},{port},\"{password}\"");
                if let Some(sni) = sni {
                    let _ = write!(line, ",tls-name={sni}");
                }
                if let Some(fp) = cert_sha256 {
                    let _ = write!(line, ",skip-cert-verify=true,tls-cert-sha256={fp}");
                }
                if let Some(obfs) = obfs_password {
                    let _ = write!(line, ",salamander-password={obfs}");
                }
                line.push_str(",udp=true");
                line
            } else {
                // Surge 不支持 salamander 混淆，能力表已经过滤
                let mut line =
                    format!("{name} = hysteria2, {host}, {port}, password=\"{password}\"");
                if let Some(sni) = sni {
                    let _ = write!(line, ", sni={sni}");
                }
                line.push_str(&pin(cert_sha256, "server-cert-fingerprint-sha256"));
                if let Some((start, end)) = ports {
                    let _ = write!(line, ", port-hopping=\"{start}-{end}\"");
                }
                line
            }
        }
        Proto::AnyTls {
            password,
            sni,
            cert_sha256,
        } => {
            if loon {
                let mut line = format!("{name} = anytls,{host},{port},\"{password}\"");
                if let Some(sni) = sni {
                    let _ = write!(line, ",tls-name={sni}");
                }
                if let Some(fp) = cert_sha256 {
                    let _ = write!(line, ",skip-cert-verify=true,tls-cert-sha256={fp}");
                }
                line.push_str(",udp=true");
                line
            } else {
                let mut line = format!("{name} = anytls, {host}, {port}, password=\"{password}\"");
                if let Some(sni) = sni {
                    let _ = write!(line, ", sni={sni}");
                }
                line.push_str(&pin(cert_sha256, "server-cert-fingerprint-sha256"));
                line
            }
        }
        Proto::Ss { method, password } => {
            if loon {
                format!("{name} = shadowsocks,{host},{port},{method},\"{password}\",udp=true")
            } else {
                format!(
                    "{name} = ss, {host}, {port}, encrypt-method={method}, password=\"{password}\", udp-relay=true"
                )
            }
        }
        _ => return None,
    })
}

/// Quantumult X 的一个节点（[server_local]）。
fn qx_line(node: &ProxyNode) -> Option<String> {
    let (name, host, port) = (&node.name, &node.server, node.port);
    Some(match &node.proto {
        Proto::Vless {
            uuid,
            public_key,
            short_id,
            sni,
        } => format!(
            "vless={host}:{port}, method=none, password={uuid}, obfs=over-tls, tls-host={sni}, vless-flow=xtls-rprx-vision, udp-relay=true, reality-base64-pubkey={public_key}, reality-hex-shortid={short_id}, tag={name}"
        ),
        Proto::AnyTls {
            password,
            sni,
            cert_sha256,
        } => {
            let mut line = format!("anytls={host}:{port}, password={password}, over-tls=true");
            if let Some(sni) = sni {
                let _ = write!(line, ", tls-host={sni}");
            }
            if let Some(fp) = cert_sha256 {
                let _ = write!(line, ", tls-verification=false, tls-cert-sha256={fp}");
            }
            let _ = write!(line, ", udp-relay=true, tag={name}");
            line
        }
        Proto::Ss { method, password } => format!(
            "shadowsocks={host}:{port}, method={method}, password={password}, udp-relay=true, tag={name}"
        ),
        _ => return None,
    })
}

/// 规则的策略名：Quantumult X 的内置策略是小写。
fn policy_name(policy: &str, format: Format) -> String {
    match (format, policy) {
        (Format::QuantumultX, "DIRECT") => "direct".to_string(),
        (Format::QuantumultX, "REJECT") => "reject".to_string(),
        _ => policy.to_string(),
    }
}

fn surge_group(g: &Group, format: Format) -> String {
    let kind = if GROUP_KINDS.contains(&g.kind.as_str()) {
        g.kind.as_str()
    } else {
        "select"
    };
    let (sep, eq) = if format == Format::Loon {
        (",", " = ")
    } else {
        (", ", "=")
    };
    let mut line = format!("{} = {kind}{sep}{}", g.name, g.members.join(sep));
    if kind != "select" {
        let url = g.url.as_deref().unwrap_or(TEST_URL);
        let _ = write!(
            line,
            "{sep}url{eq}{url}{sep}interval{eq}{}",
            g.interval.unwrap_or(300)
        );
        if kind == "url-test"
            && let Some(t) = g.tolerance
        {
            let _ = write!(line, "{sep}tolerance{eq}{t}");
        }
    }
    line
}

fn qx_group(g: &Group) -> String {
    let members: Vec<String> = g
        .members
        .iter()
        .map(|m| policy_name(m, Format::QuantumultX))
        .collect();
    let members = members.join(", ");
    match g.kind.as_str() {
        "url-test" => format!(
            "url-latency-benchmark={}, {members}, check-interval={}, tolerance={}",
            g.name,
            g.interval.unwrap_or(300),
            g.tolerance.unwrap_or(50)
        ),
        "fallback" => format!(
            "available={}, {members}, check-interval={}",
            g.name,
            g.interval.unwrap_or(300)
        ),
        "load-balance" => format!("round-robin={}, {members}", g.name),
        _ => format!("static={}, {members}", g.name),
    }
}

/// 规则：返回 (本地规则行, 远程规则集行)。
fn rules(tpl: &Template, format: Format) -> (Vec<String>, Vec<String>) {
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for rule in &tpl.rules {
        match rule {
            RuleDef::Simple {
                kind,
                value,
                policy,
                no_resolve,
            } if tpl.known_policy(policy) => {
                let policy = policy_name(policy, format);
                if format == Format::QuantumultX {
                    let qx_kind = match kind.as_str() {
                        "DOMAIN" => "host",
                        "DOMAIN-SUFFIX" => "host-suffix",
                        "DOMAIN-KEYWORD" => "host-keyword",
                        "IP-CIDR" => "ip-cidr",
                        "IP-CIDR6" => "ip6-cidr",
                        _ => "geoip",
                    };
                    local.push(format!("{qx_kind}, {value}, {policy}"));
                } else {
                    let suffix = if *no_resolve { ",no-resolve" } else { "" };
                    local.push(format!("{kind},{value},{policy}{suffix}"));
                }
            }
            RuleDef::RuleSet {
                provider, policy, ..
            } if tpl.known_policy(policy) => {
                if let Ok(p) = tpl.rule_set_support(format, provider)
                    && let Some(url) = &p.url
                {
                    let policy = policy_name(policy, format);
                    match format {
                        Format::Loon => remote.push(format!(
                            "{url}, policy={policy}, tag={provider}, enabled=true"
                        )),
                        _ => local.push(format!("RULE-SET,{url},{policy}")),
                    }
                }
            }
            RuleDef::Match { policy } if tpl.known_policy(policy) => {
                let policy = policy_name(policy, format);
                if format == Format::QuantumultX {
                    local.push(format!("final, {policy}"));
                } else {
                    local.push(format!("FINAL,{policy}"));
                }
            }
            _ => {}
        }
    }
    (local, remote)
}

/// Surge 或 Loon 的完整配置。
pub fn surge_like(
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
    let (local, remote) = rules(tpl, format);

    let mut out = String::new();
    out.push_str("[General]\n");
    if format == Format::Loon {
        out.push_str(
            "skip-proxy = 127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local\n",
        );
    } else {
        out.push_str("loglevel = notify\n");
        let _ = writeln!(out, "internet-test-url = {TEST_URL}");
        let _ = writeln!(out, "proxy-test-url = {TEST_URL}");
        out.push_str("skip-proxy = 127.0.0.1, 192.168.0.0/16, 10.0.0.0/8, 172.16.0.0/12, localhost, *.local\n");
    }
    out.push_str("\n[Proxy]\n");
    for n in hints.iter().chain(usable.iter().copied()) {
        if let Some(line) = surge_line(n, format) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("\n[Proxy Group]\n");
    for g in &groups {
        out.push_str(&surge_group(g, format));
        out.push('\n');
    }
    out.push_str("\n[Rule]\n");
    for r in &local {
        out.push_str(r);
        out.push('\n');
    }
    if format == Format::Loon && !remote.is_empty() {
        out.push_str("\n[Remote Rule]\n");
        for r in &remote {
            out.push_str(r);
            out.push('\n');
        }
    }
    out
}

/// Quantumult X 的完整配置。
pub fn quantumultx(tpl: &Template, hints: &[ProxyNode], nodes: &[ProxyNode]) -> String {
    let format = Format::QuantumultX;
    let usable: Vec<&ProxyNode> = nodes
        .iter()
        .filter(|n| supports(format, &n.proto))
        .collect();
    let names: Vec<String> = usable.iter().map(|n| n.name.clone()).collect();
    let groups = tpl.expand(&names);
    let (local, _) = rules(tpl, format);

    let mut out = String::new();
    out.push_str("[general]\n");
    let _ = writeln!(out, "server_check_url={TEST_URL}");
    out.push_str("\n[policy]\n");
    for g in &groups {
        out.push_str(&qx_group(g));
        out.push('\n');
    }
    out.push_str("\n[server_local]\n");
    for n in hints.iter().chain(usable.iter().copied()) {
        if let Some(line) = qx_line(n) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("\n[filter_local]\n");
    for r in &local {
        out.push_str(r);
        out.push('\n');
    }
    out
}
