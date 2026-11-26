//! 通过宿主 HTTP 分派查询账号和订阅，并提交仅包含权重的批量更新

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use gateway_plugin_sdk::{
    call::middleware::{MiddlewareHeader, http::Version},
    client::{HostClient, HttpBody, HttpFrame, HttpRequest},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const PAGE_SIZE: &str = "100";
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const ACCOUNT_PATH: &str = "/api/admin/accounts";
const PLUGIN_INSTANCES: &str = "/api/admin/plugins/instances";

#[derive(Debug)]
pub(crate) struct GatewayError(pub(crate) String);

#[derive(Deserialize)]
struct Envelope<T> {
    code: u16,
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageMeta {
    total_pages: u32,
}

#[derive(Deserialize)]
struct Page<T> {
    items: Vec<T>,
    page: PageMeta,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Group {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Capabilities {
    pub(crate) subscription: bool,
}

/// 权重预览使用的账号资料，宿主的 updatedAt 也会随后台额度观测而变化
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) email: Option<String>,
    pub(crate) account_id: Option<String>,
    pub(crate) plan_type: Option<String>,
    pub(crate) capabilities: Capabilities,
    pub(crate) enabled: bool,
    pub(crate) weight: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Subscription {
    pub(crate) starts_at: Option<DateTime<Utc>>,
    pub(crate) starts_at_display: Option<String>,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) expires_at_display: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginInstance {
    pub(crate) id: String,
    pub(crate) artifact_sha256: String,
    pub(crate) configuration: Value,
    pub(crate) enabled: bool,
    name: String,
    bindings: Vec<Value>,
    revision: u64,
}

pub(crate) async fn plugin_instance(
    host: &HostClient,
    instance_id: &str,
) -> Result<Option<PluginInstance>, GatewayError> {
    let instances: Vec<PluginInstance> = admin(host, PLUGIN_INSTANCES, &[], None).await?;
    Ok(instances
        .into_iter()
        .find(|instance| instance.id == instance_id))
}

pub(crate) async fn save_plugin_configuration(
    host: &HostClient,
    instance: PluginInstance,
    configuration: Value,
) -> Result<(), GatewayError> {
    let _: Value = admin(
        host,
        "/api/admin/plugins/instances/update",
        &[],
        Some(json!({
            "id": instance.id,
            "instance": {
                "artifactSha256": instance.artifact_sha256,
                "name": instance.name,
                "enabled": instance.enabled,
                "bindings": instance.bindings,
                "expectedRevision": instance.revision,
                "configuration": configuration,
            },
        })),
    )
    .await?;
    Ok(())
}

async fn admin<T: DeserializeOwned>(
    host: &HostClient,
    path: &str,
    params: &[(&str, &str)],
    body: Option<serde_json::Value>,
) -> Result<T, GatewayError> {
    let query: String = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    let uri = if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{query}")
    };
    let (method, headers, body) = match body {
        Some(value) => (
            "POST",
            vec![MiddlewareHeader {
                name: "content-type".into(),
                value: b"application/json".to_vec(),
            }],
            HttpBody::from_bytes(
                serde_json::to_vec(&value).map_err(|_| GatewayError("无法生成宿主请求".into()))?,
            ),
        ),
        None => ("GET", Vec::new(), HttpBody::empty()),
    };
    let reply = host
        .dispatch_http(HttpRequest {
            settings: serde_json::Value::Null,
            method: method.into(),
            uri,
            version: Version::Http11,
            headers,
            timeout_ms: Some(15_000),
            body,
        })
        .await
        .map_err(|_| GatewayError("宿主管理接口调用失败".into()))?;
    let mut body = reply.body;
    if !(200..300).contains(&reply.status) {
        let _ = body.close().await;
        return Err(GatewayError(format!(
            "宿主管理接口返回 HTTP {}",
            reply.status
        )));
    }
    let mut bytes = Vec::new();
    while let Some(frame) = body
        .read()
        .await
        .map_err(|_| GatewayError("读取宿主管理接口响应失败".into()))?
    {
        if let HttpFrame::Data(data) = frame {
            if data.len() > MAX_RESPONSE_BYTES - bytes.len() {
                let _ = body.close().await;
                return Err(GatewayError("宿主管理接口响应过大".into()));
            }
            bytes.extend(data);
        }
    }
    let envelope: Envelope<T> = serde_json::from_slice(&bytes)
        .map_err(|_| GatewayError("无法解析宿主管理接口响应".into()))?;
    if envelope.code != 200 {
        return Err(GatewayError(format!(
            "宿主管理接口返回业务码 {}",
            envelope.code
        )));
    }
    Ok(envelope.data)
}

async fn pages<T: DeserializeOwned>(
    host: &HostClient,
    path: &str,
    params: &[(&str, &str)],
) -> Result<Vec<T>, GatewayError> {
    let mut items = Vec::new();
    let mut page_index = 1;
    loop {
        let page_number = page_index.to_string();
        let mut query = params.to_vec();
        query.extend([("page", page_number.as_str()), ("pageSize", PAGE_SIZE)]);
        let page: Page<T> = admin(host, path, &query, None).await?;
        items.extend(page.items);
        if page_index >= page.page.total_pages {
            return Ok(items);
        }
        page_index += 1;
    }
}

pub(crate) async fn groups(host: &HostClient) -> Result<Vec<Group>, GatewayError> {
    pages(
        host,
        "/api/admin/account-groups/options",
        &[("enabled", "true")],
    )
    .await
}

pub(crate) async fn accounts(
    host: &HostClient,
    group_id: &str,
    plan_type: &str,
) -> Result<BTreeMap<String, Account>, GatewayError> {
    let accounts: Vec<Account> = pages(
        host,
        ACCOUNT_PATH,
        &[("provider", "openai"), ("groupId", group_id)],
    )
    .await?;
    let mut matching = BTreeMap::new();
    for account in accounts.into_iter().filter(|account| {
        account
            .plan_type
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(plan_type))
    }) {
        if matching.insert(account.id.clone(), account).is_some() {
            return Err(GatewayError("查询期间分组成员发生变化，请重新查询".into()));
        }
    }
    Ok(matching)
}

pub(crate) async fn subscription(
    host: &HostClient,
    id: &str,
) -> Result<Option<Subscription>, GatewayError> {
    #[derive(Deserialize)]
    struct PersonalInfo {
        subscription: Option<Subscription>,
    }
    let info: PersonalInfo = admin(
        host,
        "/api/admin/accounts/personal-info",
        &[("accountId", id)],
        None,
    )
    .await?;
    Ok(info.subscription)
}

pub(crate) async fn update_weight(
    host: &HostClient,
    ids: &[&str],
    weight: u16,
) -> Result<(), GatewayError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UpdateResult {
        account_ids: Vec<String>,
    }
    let result: UpdateResult = admin(
        host,
        "/api/admin/accounts/batch-update",
        &[],
        Some(json!({
            "accountIds": ids,
            "weight": weight,
        })),
    )
    .await?;
    if result.account_ids.len() != ids.len()
        || ids
            .iter()
            .any(|id| !result.account_ids.iter().any(|actual| actual == id))
    {
        return Err(GatewayError("宿主返回的更新账号与预览不一致".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_observation_does_not_change_the_account_snapshot() {
        let mut account = json!({
            "id": "acct_fixture",
            "name": "Fixture",
            "planType": "plus",
            "capabilities": { "subscription": true },
            "enabled": true,
            "weight": 40,
            "updatedAt": "2026-11-26T00:00:00Z",
        });
        let preview: Account = serde_json::from_value(account.clone()).unwrap();

        account["updatedAt"] = json!("2026-11-26T00:01:00Z");
        let current: Account = serde_json::from_value(account).unwrap();

        assert_eq!(preview, current);
    }
}
