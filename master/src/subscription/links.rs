//! 通用分享链接（v2rayN、v2rayNG）：每行一个链接，整体 base64，只有节点。

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::node::{Format, Proto, ProxyNode, supports};

/// 节点名和参数值：按 encodeURIComponent 的规则编码（空格是 %20）。
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Shadowsocks 2022 的 userinfo 不做 base64，百分号编码但保留 + = : 等合法字符
/// （参考笔记：有的客户端不解码百分号，会把编码后的文本当密码）。
const USERINFO: &AsciiSet = &COMPONENT
    .remove(b'+')
    .remove(b'=')
    .remove(b':')
    .remove(b'/');

fn enc(s: &str) -> String {
    utf8_percent_encode(s, COMPONENT).to_string()
}

/// 自签证书的指纹写成 Hysteria 官方的格式：大写、冒号分隔。
fn colon_hex(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|c| String::from_utf8_lossy(c).to_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

fn link(node: &ProxyNode) -> Option<String> {
    let (host, port, name) = (&node.server, node.port, enc(&node.name));
    Some(match &node.proto {
        Proto::Vless {
            uuid,
            public_key,
            short_id,
            sni,
        } => format!(
            "vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{name}",
            enc(sni),
            enc(public_key),
            enc(short_id)
        ),
        Proto::Hysteria2 {
            password,
            sni,
            obfs_password,
            ports,
            cert_sha256,
        } => {
            let mut params = Vec::new();
            if let Some(sni) = sni {
                params.push(format!("sni={}", enc(sni)));
            }
            if let Some(obfs) = obfs_password {
                params.push("obfs=salamander".to_string());
                params.push(format!("obfs-password={}", enc(obfs)));
            }
            if let Some(fp) = cert_sha256 {
                params.push("insecure=1".to_string());
                params.push(format!("pinSHA256={}", enc(&colon_hex(fp))));
            }
            if let Some((start, end)) = ports {
                params.push(format!("mport={start}-{end}"));
            }
            format!(
                "hysteria2://{}@{host}:{port}/?{}#{name}",
                enc(password),
                params.join("&")
            )
        }
        Proto::AnyTls {
            password,
            sni,
            cert_sha256,
        } => {
            let mut params = vec!["udp=1".to_string()];
            if let Some(sni) = sni {
                params.push(format!("sni={}", enc(sni)));
            }
            if cert_sha256.is_some() {
                params.push("insecure=1".to_string());
            }
            format!(
                "anytls://{}@{host}:{port}/?{}#{name}",
                enc(password),
                params.join("&")
            )
        }
        Proto::Ss { method, password } => {
            let plain = format!("{method}:{password}");
            // SIP002：2022 系用百分号编码的明文，老的加密方式（提示节点用的 aes-128-gcm）用 base64url
            let userinfo = if method.starts_with("2022-") {
                utf8_percent_encode(&plain, USERINFO).to_string()
            } else {
                URL_SAFE_NO_PAD.encode(plain)
            };
            format!("ss://{userinfo}@{host}:{port}#{name}")
        }
        Proto::Mieru { username, password } => format!(
            "mieru://{}:{}@{host}:{port}/?transport=TCP#{name}",
            enc(username),
            enc(password)
        ),
    })
}

pub fn render(hints: &[ProxyNode], nodes: &[ProxyNode]) -> String {
    let lines: Vec<String> = hints
        .iter()
        .chain(nodes.iter().filter(|n| supports(Format::V2ray, &n.proto)))
        .filter_map(link)
        .collect();
    STANDARD.encode(lines.join("\n"))
}
