use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const METHOD_GET_USAGE_SUMMARY: &str = "GET_USAGE_SUMMARY";
pub const METHOD_GET_APP_BREAKDOWN: &str = "GET_APP_BREAKDOWN";
pub const METHOD_GET_DAEMON_STATUS: &str = "GET_DAEMON_STATUS";
pub const METHOD_SET_SETTINGS: &str = "SET_SETTINGS";
pub const METHOD_GET_SETTINGS: &str = "GET_SETTINGS";
pub const METHOD_GET_AFK_AUDIT: &str = "GET_AFK_AUDIT";
pub const METHOD_UPSERT_AFK_WINDOW: &str = "UPSERT_AFK_WINDOW";
pub const METHOD_SUBSCRIBE_EVENTS: &str = "SUBSCRIBE_EVENTS";
pub const METHOD_INGEST_ATTRIBUTED_USAGE: &str = "INGEST_ATTRIBUTED_USAGE";
pub const METHOD_SET_IMPORT_STATUS: &str = "SET_IMPORT_STATUS";
pub const METHOD_GET_INTERFACES: &str = "GET_INTERFACES";
pub const METHOD_GET_INTERFACE_BREAKDOWN: &str = "GET_INTERFACE_BREAKDOWN";
pub const METHOD_LIST_CAP_DEFINITIONS: &str = "LIST_CAP_DEFINITIONS";
pub const METHOD_UPSERT_CAP_DEFINITION: &str = "UPSERT_CAP_DEFINITION";
pub const METHOD_DELETE_CAP_DEFINITION: &str = "DELETE_CAP_DEFINITION";
pub const METHOD_LIST_CAP_ALERT_EVENTS: &str = "LIST_CAP_ALERT_EVENTS";
pub const METHOD_MARK_CAP_ALERT_EVENTS_DELIVERED: &str = "MARK_CAP_ALERT_EVENTS_DELIVERED";
pub const METHOD_COMPACT_DATABASE: &str = "COMPACT_DATABASE";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Request,
    Response,
    Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcMessage {
    pub id: Option<Uuid>,
    #[serde(rename = "type")]
    pub message_type: MessageType,
    pub method: String,
    pub payload: Value,
    pub error: Option<IpcError>,
}

impl IpcMessage {
    pub fn request(method: impl Into<String>, payload: impl Serialize) -> serde_json::Result<Self> {
        Ok(Self {
            id: Some(Uuid::new_v4()),
            message_type: MessageType::Request,
            method: method.into(),
            payload: serde_json::to_value(payload)?,
            error: None,
        })
    }

    pub fn response<T: Serialize>(req: &IpcMessage, payload: T) -> serde_json::Result<Self> {
        Ok(Self {
            id: req.id,
            message_type: MessageType::Response,
            method: req.method.clone(),
            payload: serde_json::to_value(payload)?,
            error: None,
        })
    }

    pub fn error_response(req: &IpcMessage, code: i32, message: impl Into<String>) -> Self {
        Self {
            id: req.id,
            message_type: MessageType::Response,
            method: req.method.clone(),
            payload: Value::Null,
            error: Some(IpcError {
                code,
                message: message.into(),
            }),
        }
    }

    pub fn event(method: impl Into<String>, payload: impl Serialize) -> serde_json::Result<Self> {
        Ok(Self {
            id: None,
            message_type: MessageType::Event,
            method: method.into(),
            payload: serde_json::to_value(payload)?,
            error: None,
        })
    }

    pub fn from_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line)
    }

    pub fn to_line(&self) -> serde_json::Result<String> {
        let mut text = serde_json::to_string(self)?;
        text.push('\n');
        Ok(text)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRangeRequest {
    pub start_ts: i64,
    pub end_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummaryRequest {
    pub start_ts: i64,
    pub end_ts: i64,
    pub granularity: String,
    pub interface_id: Option<String>,
    pub interface_type: Option<String>,
    pub app_filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBucket {
    pub ts: i64,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub interface_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummaryResponse {
    pub buckets: Vec<UsageBucket>,
    pub total_sent: u64,
    pub total_recv: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppBreakdownRequest {
    pub start_ts: i64,
    pub end_ts: i64,
    pub interface_id: Option<String>,
    pub interface_type: Option<String>,
    pub limit: Option<u32>,
    pub sort_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUsageRow {
    pub process_name: String,
    pub display_name: String,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub last_seen_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppBreakdownResponse {
    pub apps: Vec<AppUsageRow>,
    pub total_apps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub guid: String,
    pub name: String,
    pub interface_type: String,
    pub is_metered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetInterfacesResponse {
    pub interfaces: Vec<InterfaceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceBreakdownRequest {
    pub start_ts: i64,
    pub end_ts: i64,
    pub interface_id: Option<String>,
    pub interface_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceUsageRow {
    pub interface_id: String,
    pub interface_name: String,
    pub interface_type: String,
    pub is_metered: bool,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceBreakdownResponse {
    pub interfaces: Vec<InterfaceUsageRow>,
    pub total_interfaces: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapDefinition {
    pub id: i64,
    pub scope: String,
    pub interface_guid: Option<String>,
    pub monthly_cap_bytes: u64,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCapDefinitionsResponse {
    pub caps: Vec<CapDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertCapDefinitionRequest {
    pub id: Option<i64>,
    pub scope: String,
    pub interface_guid: Option<String>,
    pub monthly_cap_bytes: u64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertCapDefinitionResponse {
    pub cap: CapDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCapDefinitionRequest {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteCapDefinitionResponse {
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCapAlertEventsRequest {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub scope: Option<String>,
    pub interface_guid: Option<String>,
    pub window_kind: Option<String>,
    pub threshold_kind: Option<String>,
    pub delivery_state: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapAlertEvent {
    pub id: i64,
    pub cap_definition_id: i64,
    pub scope: String,
    pub interface_guid: Option<String>,
    pub window_kind: String,
    pub window_start_ts: i64,
    pub window_end_ts: i64,
    pub threshold_kind: String,
    pub threshold_value: u64,
    pub usage_bytes: u64,
    pub cap_bytes: u64,
    pub fired_at: i64,
    pub delivery_state: String,
    pub delivered_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCapAlertEventsResponse {
    pub events: Vec<CapAlertEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkCapAlertEventsDeliveredRequest {
    pub event_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkCapAlertEventsDeliveredResponse {
    pub updated: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactDatabaseRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactDatabaseResponse {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub reclaimed_bytes: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatusResponse {
    pub version: String,
    pub uptime_seconds: u64,
    pub memory_bytes: u64,
    pub cpu_percent_1m: f64,
    pub last_poll_ts: i64,
    pub next_poll_ts: i64,
    pub poll_interval_seconds: u32,
    pub db_size_bytes: u64,
    pub import_status: String,
    pub import_progress_pct: u8,
    pub attribution_mode: String,
    pub last_helper_ingest_ts: i64,
    pub retention_cleanup_last_run_ts: i64,
    pub retention_cleanup_cutoff_ts: i64,
    pub retention_cleanup_deleted_usage_records: u64,
    pub retention_cleanup_deleted_afk_windows: u64,
    pub retention_cleanup_last_result: String,
    pub daemon_start_count: u64,
    pub daemon_clean_exit_count: u64,
    pub daemon_unexpected_exit_count: u64,
    pub daemon_last_start_ts: i64,
    pub daemon_last_exit_ts: i64,
    pub daemon_last_error_ts: i64,
    pub daemon_last_error_stage: String,
    pub daemon_last_error_message: String,
    pub poll_error_count: u64,
    pub ipc_error_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSettingsRequest {
    pub poll_interval_seconds: Option<u32>,
    pub retention_days: Option<u32>,
    pub afk_idle_threshold_seconds: Option<u32>,
    pub onboarding_completed: Option<bool>,
    pub export_default_granularity: Option<String>,
    pub export_default_include_summary: Option<bool>,
    pub export_default_include_apps: Option<bool>,
    pub export_default_include_interfaces: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub poll_interval_seconds: u32,
    pub retention_days: u32,
    pub afk_idle_threshold_seconds: u32,
    pub onboarding_completed: bool,
    pub export_default_granularity: String,
    pub export_default_include_summary: bool,
    pub export_default_include_apps: bool,
    pub export_default_include_interfaces: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetImportStatusRequest {
    pub status: String,
    pub progress_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AfkWindowUsage {
    pub start_ts: i64,
    pub end_ts: i64,
    pub duration_seconds: u32,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub top_apps: Vec<AppUsageRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAfkAuditRequest {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AfkAuditResponse {
    pub afk_windows: Vec<AfkWindowUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributedUsageSample {
    pub attribution_id: String,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestAttributedUsageRequest {
    pub start_ts: i64,
    pub end_ts: i64,
    pub profile_name: Option<String>,
    pub source: Option<String>,
    pub samples: Vec<AttributedUsageSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestAttributedUsageResponse {
    pub accepted: u32,
    pub dropped: u32,
}
