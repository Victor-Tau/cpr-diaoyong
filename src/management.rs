//! 管理页面入口、订阅预览与经确认的权重写入

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use gateway_plugin_sdk::{
    ErrorCode, PluginFault,
    call::management::{
        ManagementPage, ManagementRegistration, ManagementRequest, ManagementResource,
        ManagementResponse, ManagementRoute,
    },
    client::{HostClient, TypedCall, TypedReply},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    gateway::{self, Account, GatewayError, Group, Subscription},
    ranking::{self, Order, Status, WeightPolicy},
};

const JSON: &str = "application/json";
const PREVIEW_LIFETIME: Duration = Duration::from_secs(600);
const MAX_PREVIEWS: usize = 32;
const SUBSCRIPTION_CONCURRENCY: usize = 8;

pub(crate) fn registration() -> ManagementRegistration {
    ManagementRegistration {
        routes: [("GET", "options"), ("POST", "preview"), ("POST", "apply")]
            .into_iter()
            .map(|(method, path)| ManagementRoute {
                method: method.into(),
                path: path.into(),
                request_content_types: if method == "POST" {
                    vec![JSON.into()]
                } else {
                    Vec::new()
                },
                response_content_types: vec![JSON.into()],
            })
            .collect(),
        resources: ["web/index.html", "web/app.js", "web/app.css"]
            .into_iter()
            .map(|path| ManagementResource {
                path: path.into(),
                public: false,
            })
            .collect(),
        pages: vec![ManagementPage {
            id: "subscription-weights".into(),
            title: "订阅权重编排".into(),
            description: None,
            entry: "web/index.html".into(),
            icon: None,
        }],
        callbacks: Vec::new(),
    }
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Configuration {
    group_ids: String,
    plan_types: String,
    excluded_account_ids: String,
    min_weight: u16,
    max_weight: u16,
    ranking_after_days: u16,
    initial_weight: Option<u16>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            group_ids: String::new(),
            plan_types: "plus".into(),
            excluded_account_ids: String::new(),
            min_weight: 1,
            max_weight: 100,
            ranking_after_days: 0,
            initial_weight: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewRequest {
    group_id: String,
    plan_type: String,
    order: Order,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyRequest {
    preview_id: Uuid,
    account_ids: Vec<String>,
}

struct Preview {
    created_at: Instant,
    valid_until: Option<DateTime<Utc>>,
    request: PreviewRequest,
    accounts: BTreeMap<String, Account>,
    weights: BTreeMap<String, u16>,
}

impl Preview {
    fn is_valid(&self) -> bool {
        self.created_at.elapsed() < PREVIEW_LIFETIME
            && self
                .valid_until
                .is_none_or(|deadline| deadline > Utc::now())
    }
}

pub(crate) struct App {
    group_ids: BTreeSet<String>,
    plan_types: Vec<String>,
    excluded_account_ids: BTreeSet<String>,
    weight_policy: Result<WeightPolicy, &'static str>,
    previews: Mutex<BTreeMap<Uuid, Preview>>,
    applying: Mutex<()>,
}

struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<GatewayError> for ApiError {
    fn from(error: GatewayError) -> Self {
        Self::new(502, error.0)
    }
}

type Response = (u16, Value);

fn split_setting(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', '，', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn parse<T: DeserializeOwned>(payload: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(payload).map_err(|_| ApiError::new(400, "请求参数无效"))
}

fn ok(body: Value) -> Response {
    (200, body)
}

impl App {
    pub(crate) fn new(config: Configuration) -> Self {
        let mut seen = BTreeSet::new();
        let plan_types = split_setting(&config.plan_types)
            .map(str::to_ascii_lowercase)
            .filter(|plan| seen.insert(plan.clone()))
            .collect();
        Self {
            group_ids: split_setting(&config.group_ids)
                .map(str::to_owned)
                .collect(),
            plan_types,
            excluded_account_ids: split_setting(&config.excluded_account_ids)
                .map(str::to_owned)
                .collect(),
            weight_policy: WeightPolicy::new(
                config.min_weight,
                config.max_weight,
                config.ranking_after_days,
                config.initial_weight,
            ),
            previews: Mutex::default(),
            applying: Mutex::default(),
        }
    }

    pub(crate) async fn handle(
        &self,
        call: TypedCall<ManagementRequest>,
    ) -> Result<TypedReply<ManagementResponse>, PluginFault> {
        let result = match (call.request.method.as_str(), call.request.path.as_str()) {
            ("GET", "options") => self.options(&call.host).await,
            ("POST", "preview") => match parse(&call.payload) {
                Ok(request) => self.preview(&call.host, request).await,
                Err(error) => Err(error),
            },
            ("POST", "apply") => match parse(&call.payload) {
                Ok(request) => self.apply(&call.host, request).await,
                Err(error) => Err(error),
            },
            _ => Err(ApiError::new(404, "管理接口不存在")),
        };
        let (status, body) =
            result.unwrap_or_else(|error| (error.status, json!({ "message": error.message })));
        let payload = serde_json::to_vec(&body)
            .map_err(|_| PluginFault::new(ErrorCode::Fault, "无法生成管理接口响应"))?;
        Ok(TypedReply::new(ManagementResponse {
            status,
            content_type: JSON.into(),
            headers: Vec::new(),
        })
        .with_payload(payload))
    }

    async fn available_groups(&self, host: &HostClient) -> Result<Vec<Group>, GatewayError> {
        let mut groups = gateway::groups(host).await?;
        if !self.group_ids.is_empty() {
            groups.retain(|group| self.group_ids.contains(&group.id));
        }
        Ok(groups)
    }

    async fn options(&self, host: &HostClient) -> Result<Response, ApiError> {
        let weight_policy = self
            .weight_policy
            .map_err(|message| ApiError::new(400, message))?;
        Ok(ok(json!({
            "groups": self.available_groups(host).await?,
            "planTypes": self.plan_types,
            "weightPolicy": weight_policy,
        })))
    }

    async fn available_group(&self, host: &HostClient, group_id: &str) -> Result<(), ApiError> {
        if !self
            .available_groups(host)
            .await?
            .iter()
            .any(|group| group.id == group_id)
        {
            return Err(ApiError::new(409, "该分组已停用、删除或移出配置范围"));
        }
        Ok(())
    }

    async fn query_account<'a>(
        &self,
        host: &HostClient,
        account: &'a Account,
    ) -> (&'a Account, Result<Subscription, Status>) {
        let period = if self.excluded_account_ids.contains(&account.id) {
            Err(Status::Excluded)
        } else if !account.enabled {
            Err(Status::Disabled)
        } else if !account.capabilities.subscription {
            Err(Status::Unsupported)
        } else {
            gateway::subscription(host, &account.id)
                .await
                .ok()
                .flatten()
                .ok_or(Status::Unknown)
        };
        (account, period)
    }

    async fn preview(
        &self,
        host: &HostClient,
        request: PreviewRequest,
    ) -> Result<Response, ApiError> {
        let weight_policy = self
            .weight_policy
            .map_err(|message| ApiError::new(400, message))?;
        if !self.plan_types.contains(&request.plan_type) {
            return Err(ApiError::new(400, "该套餐不在插件配置范围内"));
        }
        self.available_group(host, &request.group_id).await?;
        let accounts = gateway::accounts(host, &request.group_id, &request.plan_type).await?;
        let queries: Vec<_> = accounts
            .values()
            .map(|account| self.query_account(host, account))
            .collect();
        let results = stream::iter(queries)
            .buffer_unordered(SUBSCRIPTION_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        let rows = ranking::rank(&results, request.order, Utc::now(), weight_policy);
        let weights: BTreeMap<_, _> = rows
            .iter()
            .filter_map(|row| {
                let weight = row.suggested_weight?;
                (weight != row.current_weight).then(|| (row.id.to_owned(), weight))
            })
            .collect();
        if weights.is_empty() {
            return Ok(ok(json!({ "previewId": null, "rows": rows })));
        }
        let preview_id = Uuid::new_v4();
        let body = json!({ "previewId": preview_id, "rows": rows });
        let valid_until = rows.iter().filter_map(|row| row.valid_until).min();
        let mut previews = self.previews.lock().await;
        previews.retain(|_, preview| preview.is_valid());
        if previews.len() >= MAX_PREVIEWS
            && let Some((id, _)) = previews
                .iter()
                .min_by_key(|(_, preview)| preview.created_at)
        {
            let id = *id;
            previews.remove(&id);
        }
        previews.insert(
            preview_id,
            Preview {
                created_at: Instant::now(),
                valid_until,
                request,
                accounts,
                weights,
            },
        );
        Ok(ok(body))
    }

    async fn apply(&self, host: &HostClient, request: ApplyRequest) -> Result<Response, ApiError> {
        let _applying = self
            .applying
            .try_lock()
            .map_err(|_| ApiError::new(409, "正在应用另一份预览，请稍后重新查询"))?;
        let preview = self
            .previews
            .lock()
            .await
            .remove(&request.preview_id)
            .filter(Preview::is_valid)
            .ok_or_else(|| ApiError::new(409, "预览已失效，请重新查询"))?;
        let account_ids: BTreeSet<_> = request.account_ids.iter().map(String::as_str).collect();
        if account_ids.is_empty() || account_ids.len() != request.account_ids.len() {
            return Err(ApiError::new(400, "请选择要应用的账号"));
        }
        let mut batches: BTreeMap<u16, Vec<&str>> = BTreeMap::new();
        for account_id in account_ids {
            let weight = preview
                .weights
                .get(account_id)
                .ok_or_else(|| ApiError::new(400, "所选账号不在预览的变更范围内"))?;
            batches.entry(*weight).or_default().push(account_id);
        }

        self.available_group(host, &preview.request.group_id)
            .await?;
        let current =
            gateway::accounts(host, &preview.request.group_id, &preview.request.plan_type).await?;
        if current != preview.accounts || !preview.is_valid() {
            return Err(ApiError::new(
                409,
                "账号资料、分组成员、权重或订阅状态已变化，请重新查询",
            ));
        }

        let mut applied = 0;
        for (weight, ids) in batches.into_iter().rev() {
            for batch in ids.chunks(1000) {
                if let Err(error) = gateway::update_weight(host, batch, weight).await {
                    return Ok((
                        502,
                        json!({ "message": format!("已确认更新 {applied} 个账号，后续批次未确认：{}。请重新查询当前权重", error.0), "applied": applied }),
                    ));
                }
                applied += batch.len();
            }
        }
        Ok(ok(json!({ "applied": applied })))
    }
}
