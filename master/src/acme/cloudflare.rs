//! Cloudflare DNS：只用到按名字找 Zone、加删 TXT 记录（DNS 服务商第一版只支持 Cloudflare）。

use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use serde::Deserialize;
use serde_json::json;

const API: &str = "https://api.cloudflare.com/client/v4";

pub struct Client {
    http: reqwest::Client,
    token: String,
}

/// 加上的一条记录，删除时用。
pub struct Record {
    zone_id: String,
    id: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
}

impl Client {
    pub fn new(token: &str) -> anyhow::Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            token: token.to_string(),
        })
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> anyhow::Result<T> {
        let resp = req
            .bearer_auth(&self.token)
            .send()
            .await
            .context("请求 Cloudflare API")?;
        let status = resp.status();
        let body: Envelope<T> = resp
            .json()
            .await
            .with_context(|| format!("解析 Cloudflare 的回复（HTTP {status}）"))?;
        if !body.success {
            bail!("Cloudflare 返回错误: {:?}", body.errors);
        }
        body.result
            .ok_or_else(|| anyhow!("Cloudflare 的回复里没有结果"))
    }

    /// 找域名所在的 Zone：从完整域名开始逐级去掉最左边一段，直到找到。
    async fn zone_of(&self, name: &str) -> anyhow::Result<String> {
        let mut candidate = name.trim_end_matches('.');
        loop {
            let zones: Vec<Item> = self
                .call(
                    self.http
                        .get(format!("{API}/zones"))
                        .query(&[("name", candidate)]),
                )
                .await?;
            if let Some(zone) = zones.into_iter().next() {
                return Ok(zone.id);
            }
            match candidate.split_once('.') {
                Some((_, rest)) if rest.contains('.') => candidate = rest,
                _ => bail!(
                    "Cloudflare 账号里找不到「{name}」所在的域名（Token 要有 Zone:Read 和 DNS:Edit 权限）"
                ),
            }
        }
    }

    pub async fn create_txt(&self, name: &str, value: &str) -> anyhow::Result<Record> {
        let zone_id = self.zone_of(name).await?;
        let item: Item = self
            .call(
                self.http
                    .post(format!("{API}/zones/{zone_id}/dns_records"))
                    .json(&json!({ "type": "TXT", "name": name, "content": value, "ttl": 60 })),
            )
            .await
            .context("添加 TXT 记录")?;
        Ok(Record {
            zone_id,
            id: item.id,
        })
    }

    pub async fn delete(&self, record: &Record) -> anyhow::Result<()> {
        let _: Item = self
            .call(self.http.delete(format!(
                "{API}/zones/{}/dns_records/{}",
                record.zone_id, record.id
            )))
            .await?;
        Ok(())
    }
}
