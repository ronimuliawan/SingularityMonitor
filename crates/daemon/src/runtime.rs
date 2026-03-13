use crate::config::AppConfig;
use crate::db::Db;
use crate::delta::DeltaEngine;
use crate::ipc::{self, IpcRequestHandler};
use crate::memory;
use crate::poller;
use crate::time::unix_timestamp;
use anyhow::Result;
use shared_contracts::{
    AppBreakdownRequest, CompactDatabaseRequest, DaemonStatusResponse, DeleteCapDefinitionRequest,
    GetAfkAuditRequest, GetAnomaliesRequest, GetForecastRequest, IngestAttributedUsageRequest,
    InterfaceBreakdownRequest, IpcMessage, ListCapAlertEventsRequest,
    MarkCapAlertEventsDeliveredRequest, SetImportStatusRequest, SetSettingsRequest,
    TimeRangeRequest, UpsertCapDefinitionRequest, UsageHeatmapRequest, UsageSummaryRequest,
};
use std::fs::OpenOptions;
use std::mem::size_of;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub started_at: Instant,
    pub last_poll_ts: i64,
    pub next_poll_ts: i64,
    pub poll_interval_seconds: u32,
    pub memory_bytes: u64,
    pub cpu_percent_1m: f64,
}

#[derive(Clone)]
pub struct RuntimeContext {
    db: Db,
    state: Arc<Mutex<RuntimeState>>,
    attribution_mode: String,
}

impl RuntimeContext {
    fn daemon_status(&self) -> DaemonStatusResponse {
        let state = self.state.lock().expect("runtime state poisoned").clone();
        let last_helper_ingest_ts = self.db.last_helper_ingest_ts().unwrap_or(0);
        let (import_status, import_progress_pct) = self
            .db
            .get_import_status()
            .unwrap_or_else(|_| ("idle".to_string(), 0));
        let retention_cleanup = self.db.get_retention_cleanup_status().unwrap_or_else(|_| {
            crate::db::RetentionCleanupStatus {
                last_run_ts: 0,
                cutoff_ts: 0,
                deleted_usage_records: 0,
                deleted_afk_windows: 0,
                last_result: "unknown".to_string(),
            }
        });
        let reliability =
            self.db
                .get_reliability_status()
                .unwrap_or_else(|_| crate::db::ReliabilityStatus {
                    daemon_start_count: 0,
                    daemon_clean_exit_count: 0,
                    daemon_unexpected_exit_count: 0,
                    daemon_last_start_ts: 0,
                    daemon_last_exit_ts: 0,
                    daemon_last_error_ts: 0,
                    daemon_last_error_stage: String::new(),
                    daemon_last_error_message: String::new(),
                    poll_error_count: 0,
                    ipc_error_count: 0,
                });
        DaemonStatusResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: state.started_at.elapsed().as_secs(),
            memory_bytes: state.memory_bytes,
            cpu_percent_1m: state.cpu_percent_1m,
            last_poll_ts: state.last_poll_ts,
            next_poll_ts: state.next_poll_ts,
            poll_interval_seconds: state.poll_interval_seconds,
            db_size_bytes: self.db.db_size_bytes(),
            import_status,
            import_progress_pct,
            attribution_mode: self.attribution_mode.clone(),
            last_helper_ingest_ts,
            retention_cleanup_last_run_ts: retention_cleanup.last_run_ts,
            retention_cleanup_cutoff_ts: retention_cleanup.cutoff_ts,
            retention_cleanup_deleted_usage_records: retention_cleanup.deleted_usage_records,
            retention_cleanup_deleted_afk_windows: retention_cleanup.deleted_afk_windows,
            retention_cleanup_last_result: retention_cleanup.last_result,
            daemon_start_count: reliability.daemon_start_count,
            daemon_clean_exit_count: reliability.daemon_clean_exit_count,
            daemon_unexpected_exit_count: reliability.daemon_unexpected_exit_count,
            daemon_last_start_ts: reliability.daemon_last_start_ts,
            daemon_last_exit_ts: reliability.daemon_last_exit_ts,
            daemon_last_error_ts: reliability.daemon_last_error_ts,
            daemon_last_error_stage: reliability.daemon_last_error_stage,
            daemon_last_error_message: reliability.daemon_last_error_message,
            poll_error_count: reliability.poll_error_count,
            ipc_error_count: reliability.ipc_error_count,
        }
    }

    fn on_ipc_transport_error(&self, message: &str) {
        if let Err(error) = self.db.increment_ipc_error_count(unix_timestamp(), message) {
            warn!("failed to record ipc transport error: {error:#}");
        }
    }
}

impl IpcRequestHandler for RuntimeContext {
    fn handle(&self, request: IpcMessage) -> IpcMessage {
        match request.method.as_str() {
            shared_contracts::METHOD_GET_DAEMON_STATUS => {
                match IpcMessage::response(&request, self.daemon_status()) {
                    Ok(response) => response,
                    Err(error) => {
                        IpcMessage::error_response(&request, 500, format!("encode failed: {error}"))
                    }
                }
            }
            shared_contracts::METHOD_GET_SETTINGS => match self.db.get_settings() {
                Ok(payload) => IpcMessage::response(&request, payload).unwrap_or_else(|error| {
                    IpcMessage::error_response(&request, 500, format!("encode failed: {error}"))
                }),
                Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
            },
            shared_contracts::METHOD_GET_USAGE_SUMMARY => {
                let parsed = serde_json::from_value::<UsageSummaryRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_usage_summary(&payload) {
                        Ok(summary) => {
                            IpcMessage::response(&request, summary).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_GET_USAGE_HEATMAP => {
                let parsed = serde_json::from_value::<UsageHeatmapRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_usage_heatmap(&payload) {
                        Ok(heatmap) => {
                            IpcMessage::response(&request, heatmap).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_GET_FORECAST => {
                let parsed = serde_json::from_value::<GetForecastRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_forecast(&payload) {
                        Ok(forecast) => {
                            IpcMessage::response(&request, forecast).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_GET_ANOMALIES => {
                let parsed = serde_json::from_value::<GetAnomaliesRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_anomalies(&payload) {
                        Ok(anomalies) => {
                            IpcMessage::response(&request, anomalies).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_GET_APP_BREAKDOWN => {
                let parsed = serde_json::from_value::<AppBreakdownRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_app_breakdown(&payload) {
                        Ok(summary) => {
                            IpcMessage::response(&request, summary).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_GET_INTERFACES => match self.db.query_interfaces() {
                Ok(payload) => IpcMessage::response(&request, payload).unwrap_or_else(|error| {
                    IpcMessage::error_response(&request, 500, format!("encode failed: {error}"))
                }),
                Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
            },
            shared_contracts::METHOD_GET_INTERFACE_BREAKDOWN => {
                let parsed =
                    serde_json::from_value::<InterfaceBreakdownRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_interface_breakdown(&payload) {
                        Ok(summary) => {
                            IpcMessage::response(&request, summary).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_LIST_CAP_DEFINITIONS => match self.db.list_cap_definitions() {
                Ok(payload) => IpcMessage::response(&request, payload).unwrap_or_else(|error| {
                    IpcMessage::error_response(&request, 500, format!("encode failed: {error}"))
                }),
                Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
            },
            shared_contracts::METHOD_LIST_CAP_ALERT_EVENTS => {
                let parsed =
                    serde_json::from_value::<ListCapAlertEventsRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.list_cap_alert_events(&payload) {
                        Ok(result) => {
                            IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_MARK_CAP_ALERT_EVENTS_DELIVERED => {
                let parsed = serde_json::from_value::<MarkCapAlertEventsDeliveredRequest>(
                    request.payload.clone(),
                );
                match parsed {
                    Ok(payload) => {
                        if payload.event_ids.is_empty() {
                            return IpcMessage::error_response(
                                &request,
                                400,
                                "event_ids must contain at least one id",
                            );
                        }

                        if payload.event_ids.iter().any(|id| *id <= 0) {
                            return IpcMessage::error_response(
                                &request,
                                400,
                                "event_ids must all be greater than 0",
                            );
                        }

                        let ts = unix_timestamp();
                        match self
                            .db
                            .mark_cap_alert_events_delivered(&payload.event_ids, ts)
                        {
                            Ok(result) => {
                                IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                    IpcMessage::error_response(
                                        &request,
                                        500,
                                        format!("encode failed: {error}"),
                                    )
                                })
                            }
                            Err(error) => {
                                IpcMessage::error_response(&request, 500, error.to_string())
                            }
                        }
                    }
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_COMPACT_DATABASE => {
                let parsed =
                    serde_json::from_value::<CompactDatabaseRequest>(request.payload.clone());
                match parsed {
                    Ok(_) => match self.db.compact_database() {
                        Ok(result) => {
                            IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_UPSERT_CAP_DEFINITION => {
                let parsed =
                    serde_json::from_value::<UpsertCapDefinitionRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => {
                        if let Err(message) = validate_upsert_cap_request(&payload) {
                            return IpcMessage::error_response(&request, 400, message);
                        }

                        let ts = unix_timestamp();
                        match self.db.upsert_cap_definition(ts, &payload) {
                            Ok(result) => {
                                if let Err(error) = self.db.evaluate_cap_alerts(ts) {
                                    warn!(
                                        "failed to evaluate cap alerts after cap upsert: {error:#}"
                                    );
                                }

                                IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                    IpcMessage::error_response(
                                        &request,
                                        500,
                                        format!("encode failed: {error}"),
                                    )
                                })
                            }
                            Err(error) => {
                                IpcMessage::error_response(&request, 500, error.to_string())
                            }
                        }
                    }
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_DELETE_CAP_DEFINITION => {
                let parsed =
                    serde_json::from_value::<DeleteCapDefinitionRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => {
                        if payload.id <= 0 {
                            return IpcMessage::error_response(
                                &request,
                                400,
                                "id must be greater than 0",
                            );
                        }

                        match self.db.delete_cap_definition(&payload) {
                            Ok(result) => {
                                IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                    IpcMessage::error_response(
                                        &request,
                                        500,
                                        format!("encode failed: {error}"),
                                    )
                                })
                            }
                            Err(error) => {
                                IpcMessage::error_response(&request, 500, error.to_string())
                            }
                        }
                    }
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_SET_SETTINGS => {
                let parsed = serde_json::from_value::<SetSettingsRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => {
                        let ts = unix_timestamp();
                        match self.db.apply_settings(ts, &payload) {
                            Ok(()) => {
                                if let Some(interval) = payload.poll_interval_seconds
                                    && let Ok(mut state) = self.state.lock()
                                {
                                    state.poll_interval_seconds = interval.clamp(15, 300);
                                }

                                IpcMessage::response(&request, serde_json::json!({ "ok": true }))
                                    .unwrap_or_else(|error| {
                                        IpcMessage::error_response(
                                            &request,
                                            500,
                                            format!("encode failed: {error}"),
                                        )
                                    })
                            }
                            Err(error) => {
                                IpcMessage::error_response(&request, 500, error.to_string())
                            }
                        }
                    }
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_GET_AFK_AUDIT => {
                let parsed = serde_json::from_value::<GetAfkAuditRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.query_afk_audit(&payload) {
                        Ok(result) => {
                            IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_UPSERT_AFK_WINDOW => {
                let parsed = serde_json::from_value::<TimeRangeRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => {
                        if payload.start_ts <= 0 || payload.end_ts < payload.start_ts {
                            return IpcMessage::error_response(&request, 400, "invalid AFK range");
                        }

                        match self.db.upsert_afk_window(
                            payload.start_ts,
                            payload.end_ts,
                            "ipc_synthetic",
                        ) {
                            Ok(()) => {
                                IpcMessage::response(&request, serde_json::json!({ "ok": true }))
                                    .unwrap_or_else(|error| {
                                        IpcMessage::error_response(
                                            &request,
                                            500,
                                            format!("encode failed: {error}"),
                                        )
                                    })
                            }
                            Err(error) => {
                                IpcMessage::error_response(&request, 500, error.to_string())
                            }
                        }
                    }
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_INGEST_ATTRIBUTED_USAGE => {
                let parsed =
                    serde_json::from_value::<IngestAttributedUsageRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => match self.db.insert_attributed_usage(&payload) {
                        Ok(result) => {
                            IpcMessage::response(&request, result).unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            })
                        }
                        Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
                    },
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            shared_contracts::METHOD_SET_IMPORT_STATUS => {
                let parsed =
                    serde_json::from_value::<SetImportStatusRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => {
                        let ts = unix_timestamp();
                        let progress_pct = payload.progress_pct.min(100);
                        match self.db.set_import_status(ts, &payload.status, progress_pct) {
                            Ok(()) => IpcMessage::response(
                                &request,
                                serde_json::json!({
                                    "ok": true,
                                    "status": payload.status,
                                    "progress_pct": progress_pct
                                }),
                            )
                            .unwrap_or_else(|error| {
                                IpcMessage::error_response(
                                    &request,
                                    500,
                                    format!("encode failed: {error}"),
                                )
                            }),
                            Err(error) => {
                                IpcMessage::error_response(&request, 500, error.to_string())
                            }
                        }
                    }
                    Err(error) => IpcMessage::error_response(
                        &request,
                        400,
                        format!("invalid payload: {error}"),
                    ),
                }
            }
            _ => IpcMessage::error_response(&request, 404, "unsupported method"),
        }
    }

    fn on_transport_error(&self, message: &str) {
        self.on_ipc_transport_error(message);
    }
}

fn validate_upsert_cap_request(
    payload: &UpsertCapDefinitionRequest,
) -> std::result::Result<(), String> {
    let scope = payload.scope.trim().to_ascii_lowercase();
    if scope != "global" && scope != "interface" {
        return Err("scope must be 'global' or 'interface'".to_string());
    }

    if payload.monthly_cap_bytes == 0 {
        return Err("monthly_cap_bytes must be greater than 0".to_string());
    }

    if payload.monthly_cap_bytes > i64::MAX as u64 {
        return Err("monthly_cap_bytes exceeds supported maximum".to_string());
    }

    if scope == "interface" {
        let has_guid = payload
            .interface_guid
            .as_deref()
            .map(str::trim)
            .map(|value| !value.is_empty())
            .unwrap_or(false);
        if !has_guid {
            return Err("interface scope requires interface_guid".to_string());
        }
    }

    if scope == "global"
        && payload
            .interface_guid
            .as_deref()
            .map(str::trim)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    {
        return Err("global scope must not include interface_guid".to_string());
    }

    if let Some(id) = payload.id
        && id <= 0
    {
        return Err("id must be greater than 0 when provided".to_string());
    }

    Ok(())
}

pub struct CollectorRuntime {
    config: AppConfig,
    db: Db,
    state: Arc<Mutex<RuntimeState>>,
    delta_engine: DeltaEngine,
}

impl CollectorRuntime {
    pub fn new(config: AppConfig) -> Result<Self> {
        let db = Db::initialize(&config.db_path)?;
        let now = unix_timestamp();
        let poll_interval = db.get_poll_interval_seconds(config.poll_interval_seconds)?;

        let state = RuntimeState {
            started_at: Instant::now(),
            last_poll_ts: 0,
            next_poll_ts: now + i64::from(poll_interval),
            poll_interval_seconds: poll_interval,
            memory_bytes: 0,
            cpu_percent_1m: 0.0,
        };

        Ok(Self {
            delta_engine: DeltaEngine::new(config.max_delta_bytes),
            config,
            db,
            state: Arc::new(Mutex::new(state)),
        })
    }

    pub fn run_once(&mut self) -> Result<()> {
        self.poll_and_store()
    }

    pub fn run(&mut self, stop_requested: Arc<AtomicBool>) -> Result<()> {
        let context = Arc::new(RuntimeContext {
            db: self.db.clone(),
            state: Arc::clone(&self.state),
            attribution_mode: self.config.attribution_mode.clone(),
        });
        let ipc_handle = ipc::spawn_server(
            self.config.pipe_name.clone(),
            context,
            Arc::clone(&stop_requested),
        );

        while !stop_requested.load(Ordering::SeqCst) {
            if let Err(error) = self.poll_and_store() {
                warn!("poll cycle failed: {error:#}");
                if let Err(record_error) = self
                    .db
                    .increment_poll_error_count(unix_timestamp(), &error.to_string())
                {
                    warn!("failed to record poll error: {record_error:#}");
                }
            }

            let interval = {
                self.db
                    .get_poll_interval_seconds(self.config.poll_interval_seconds)
                    .unwrap_or(self.config.poll_interval_seconds)
            };

            if let Ok(mut state) = self.state.lock() {
                state.poll_interval_seconds = interval;
                state.next_poll_ts = unix_timestamp() + i64::from(interval);
            }

            sleep_interruptible(interval, &stop_requested);
        }

        if let Err(error) = ipc_handle.join() {
            error!("failed to join ipc thread: {error:?}");
        }

        Ok(())
    }

    fn poll_and_store(&mut self) -> Result<()> {
        let current_ts = unix_timestamp();
        let (nominal_interval, last_poll_ts) = self
            .state
            .lock()
            .map(|s| (s.poll_interval_seconds.max(1), s.last_poll_ts))
            .unwrap_or((self.config.poll_interval_seconds.max(1), 0));
        let observed_interval =
            resolve_observed_interval_secs(last_poll_ts, current_ts, nominal_interval);

        let snapshot = poller::collect_interface_snapshot()?;
        self.db.sync_interfaces(&snapshot, current_ts)?;
        let deltas =
            self.delta_engine
                .compute(&snapshot, current_ts, observed_interval, nominal_interval);

        self.db.insert_interface_deltas(&deltas)?;
        if let Err(error) = self.db.evaluate_cap_alerts(current_ts) {
            warn!("failed to evaluate cap alerts after poll: {error:#}");
        }
        self.capture_afk_window(current_ts);
        if let Err(error) = self.db.run_retention_cleanup_if_due(current_ts) {
            warn!("failed to run retention cleanup: {error:#}");
        }
        if let Err(error) = self.db.run_hourly_aggregation_if_due(current_ts) {
            warn!("failed to run hourly aggregation: {error:#}");
        }

        if self.config.trim_working_set {
            let _ = memory::trim_working_set();
        }

        let memory_bytes = memory::current_working_set_bytes().unwrap_or(0);
        if let Ok(mut state) = self.state.lock() {
            state.last_poll_ts = current_ts;
            state.next_poll_ts = current_ts + i64::from(state.poll_interval_seconds);
            state.memory_bytes = memory_bytes;
        }

        info!(
            interface_count = snapshot.len(),
            record_count = deltas.len(),
            memory_bytes,
            "poll completed"
        );

        Ok(())
    }

    fn capture_afk_window(&self, current_ts: i64) {
        let threshold_secs = self
            .db
            .get_afk_idle_threshold_seconds(300)
            .unwrap_or(300)
            .clamp(30, 3600);

        let idle_secs = match system_idle_seconds() {
            Ok(value) => value,
            Err(error) => {
                warn!("failed to read idle signal: {error:#}");
                return;
            }
        };

        if idle_secs < threshold_secs {
            return;
        }

        let afk_start_ts = current_ts
            .saturating_sub(i64::from(idle_secs))
            .saturating_add(i64::from(threshold_secs));

        if afk_start_ts <= 0 || afk_start_ts > current_ts {
            return;
        }

        if let Err(error) = self
            .db
            .upsert_afk_window(afk_start_ts, current_ts, "last_input")
        {
            warn!("failed to persist afk window: {error:#}");
        }
    }
}

fn system_idle_seconds() -> Result<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };

    let ok = unsafe { GetLastInputInfo(&mut info) };
    if ok == 0 {
        anyhow::bail!("GetLastInputInfo returned false");
    }

    let now_ticks = unsafe { GetTickCount() };
    let idle_millis = now_ticks.wrapping_sub(info.dwTime);
    Ok(idle_millis / 1000)
}

fn resolve_observed_interval_secs(last_poll_ts: i64, current_ts: i64, fallback_secs: u32) -> u32 {
    if last_poll_ts <= 0 || current_ts <= last_poll_ts {
        return fallback_secs.max(1);
    }

    let elapsed = (current_ts - last_poll_ts) as u64;
    elapsed.clamp(1, u64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::{
        RuntimeContext, RuntimeState, resolve_observed_interval_secs, system_idle_seconds,
        validate_upsert_cap_request,
    };
    use crate::db::Db;
    use crate::ipc::IpcRequestHandler;
    use serde_json::json;
    use shared_contracts::{
        IpcMessage, METHOD_COMPACT_DATABASE, METHOD_DELETE_CAP_DEFINITION, METHOD_GET_AFK_AUDIT,
        METHOD_GET_APP_BREAKDOWN, METHOD_GET_DAEMON_STATUS, METHOD_GET_INTERFACE_BREAKDOWN,
        METHOD_GET_INTERFACES, METHOD_GET_SETTINGS, METHOD_GET_USAGE_SUMMARY,
        METHOD_INGEST_ATTRIBUTED_USAGE, METHOD_LIST_CAP_ALERT_EVENTS, METHOD_LIST_CAP_DEFINITIONS,
        METHOD_MARK_CAP_ALERT_EVENTS_DELIVERED, METHOD_SET_IMPORT_STATUS, METHOD_SET_SETTINGS,
        METHOD_UPSERT_AFK_WINDOW, METHOD_UPSERT_CAP_DEFINITION, UpsertCapDefinitionRequest,
    };
    use std::sync::{Arc, Mutex};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn new_runtime_context() -> RuntimeContext {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "singularity-monitor-runtime-test-{}-{}.db",
            std::process::id(),
            nonce
        ));
        let _ = std::fs::remove_file(&path);

        let db = Db::initialize(path).expect("initialize runtime test db");
        RuntimeContext {
            db,
            state: Arc::new(Mutex::new(RuntimeState {
                started_at: Instant::now(),
                last_poll_ts: 0,
                next_poll_ts: 0,
                poll_interval_seconds: 60,
                memory_bytes: 0,
                cpu_percent_1m: 0.0,
            })),
            attribution_mode: "helper".to_string(),
        }
    }

    #[test]
    fn ipc_handlers_accept_valid_payloads_for_supported_methods() {
        let context = new_runtime_context();

        let requests = vec![
            (METHOD_GET_DAEMON_STATUS, json!({})),
            (METHOD_GET_SETTINGS, json!({})),
            (
                METHOD_GET_USAGE_SUMMARY,
                json!({
                    "start_ts": 0,
                    "end_ts": 1,
                    "granularity": "day",
                    "interface_id": null,
                    "interface_type": null,
                    "app_filter": null
                }),
            ),
            (
                METHOD_GET_APP_BREAKDOWN,
                json!({
                    "start_ts": 0,
                    "end_ts": 1,
                    "interface_id": null,
                    "interface_type": null,
                    "limit": 10,
                    "sort_by": "total_bytes_desc"
                }),
            ),
            (
                METHOD_SET_SETTINGS,
                json!({
                    "poll_interval_seconds": 60,
                    "retention_days": 1,
                    "afk_idle_threshold_seconds": 300,
                    "onboarding_completed": false,
                    "export_default_granularity": "day",
                    "export_default_include_summary": true,
                    "export_default_include_apps": true,
                    "export_default_include_interfaces": true,
                    "cost_per_gb": 0.0
                }),
            ),
            (
                METHOD_GET_AFK_AUDIT,
                json!({"start_ts": null, "end_ts": null, "limit": 100}),
            ),
            (
                METHOD_UPSERT_AFK_WINDOW,
                json!({
                    "start_ts": 10,
                    "end_ts": 20
                }),
            ),
            (
                METHOD_INGEST_ATTRIBUTED_USAGE,
                json!({
                    "start_ts": 10,
                    "end_ts": 20,
                    "profile_name": "test",
                    "source": "helper",
                    "samples": []
                }),
            ),
            (
                METHOD_SET_IMPORT_STATUS,
                json!({
                    "status": "running",
                    "progress_pct": 10
                }),
            ),
            (METHOD_GET_INTERFACES, json!({})),
            (
                METHOD_GET_INTERFACE_BREAKDOWN,
                json!({
                    "start_ts": 0,
                    "end_ts": 100,
                    "interface_id": null,
                    "interface_type": null
                }),
            ),
            (METHOD_LIST_CAP_DEFINITIONS, json!({})),
            (
                METHOD_UPSERT_CAP_DEFINITION,
                json!({
                    "id": null,
                    "scope": "global",
                    "interface_guid": null,
                    "monthly_cap_bytes": 1000,
                    "is_active": true
                }),
            ),
            (METHOD_DELETE_CAP_DEFINITION, json!({"id": 1})),
            (
                METHOD_LIST_CAP_ALERT_EVENTS,
                json!({
                    "start_ts": null,
                    "end_ts": null,
                    "scope": null,
                    "interface_guid": null,
                    "window_kind": null,
                    "threshold_kind": null,
                    "delivery_state": null,
                    "limit": 10
                }),
            ),
            (
                METHOD_MARK_CAP_ALERT_EVENTS_DELIVERED,
                json!({"event_ids": [1]}),
            ),
            (METHOD_COMPACT_DATABASE, json!({})),
        ];

        for (method, payload) in requests {
            let request = IpcMessage::request(method, payload).expect("build ipc request");
            let response = context.handle(request);
            assert!(
                response.error.is_none(),
                "expected no error for method {method}, got {:?}",
                response.error
            );
        }
    }

    #[test]
    fn mark_cap_alert_events_delivered_rejects_invalid_event_ids() {
        let context = new_runtime_context();

        let empty_request = IpcMessage::request(
            METHOD_MARK_CAP_ALERT_EVENTS_DELIVERED,
            json!({"event_ids": []}),
        )
        .expect("build empty event_ids request");
        let empty_response = context.handle(empty_request);
        assert_eq!(
            empty_response.error.as_ref().map(|error| error.code),
            Some(400)
        );

        let non_positive_request = IpcMessage::request(
            METHOD_MARK_CAP_ALERT_EVENTS_DELIVERED,
            json!({"event_ids": [0, -1]}),
        )
        .expect("build non-positive event_ids request");
        let non_positive_response = context.handle(non_positive_request);
        assert_eq!(
            non_positive_response.error.as_ref().map(|error| error.code),
            Some(400)
        );
    }

    #[test]
    fn daemon_status_payload_includes_retention_and_reliability_fields() {
        let context = new_runtime_context();
        let request =
            IpcMessage::request(METHOD_GET_DAEMON_STATUS, json!({})).expect("build status request");
        let response = context.handle(request);
        assert!(response.error.is_none());

        let payload = response.payload;
        assert!(payload.get("retention_cleanup_last_run_ts").is_some());
        assert!(payload.get("retention_cleanup_cutoff_ts").is_some());
        assert!(
            payload
                .get("retention_cleanup_deleted_usage_records")
                .is_some()
        );
        assert!(
            payload
                .get("retention_cleanup_deleted_afk_windows")
                .is_some()
        );
        assert!(payload.get("retention_cleanup_last_result").is_some());
        assert!(payload.get("daemon_start_count").is_some());
        assert!(payload.get("daemon_clean_exit_count").is_some());
        assert!(payload.get("daemon_unexpected_exit_count").is_some());
        assert!(payload.get("daemon_last_start_ts").is_some());
        assert!(payload.get("daemon_last_exit_ts").is_some());
        assert!(payload.get("daemon_last_error_ts").is_some());
        assert!(payload.get("daemon_last_error_stage").is_some());
        assert!(payload.get("daemon_last_error_message").is_some());
        assert!(payload.get("poll_error_count").is_some());
        assert!(payload.get("ipc_error_count").is_some());
    }

    #[test]
    fn uses_fallback_when_no_previous_poll() {
        let interval = resolve_observed_interval_secs(0, 1000, 60);
        assert_eq!(interval, 60);
    }

    #[test]
    fn uses_elapsed_seconds_when_available() {
        let interval = resolve_observed_interval_secs(1000, 1125, 60);
        assert_eq!(interval, 125);
    }

    #[test]
    fn protects_against_clock_regressions() {
        let interval = resolve_observed_interval_secs(1500, 1490, 60);
        assert_eq!(interval, 60);
    }

    #[test]
    fn cap_validation_rejects_interface_scope_without_guid() {
        let result = validate_upsert_cap_request(&UpsertCapDefinitionRequest {
            id: None,
            scope: "interface".to_string(),
            interface_guid: None,
            monthly_cap_bytes: 1,
            is_active: true,
        });
        assert!(result.is_err());
    }

    #[test]
    fn cap_validation_accepts_global_scope_without_guid() {
        let result = validate_upsert_cap_request(&UpsertCapDefinitionRequest {
            id: None,
            scope: "global".to_string(),
            interface_guid: None,
            monthly_cap_bytes: 1,
            is_active: true,
        });
        assert!(result.is_ok());
    }

    #[test]
    fn idle_seconds_probe_returns_non_negative() {
        assert!(system_idle_seconds().is_ok());
    }
}

#[allow(clippy::items_after_test_module)]
pub fn init_logging(config: &AppConfig) -> Result<()> {
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.log_path)?;

    tracing_subscriber::fmt()
        .with_writer(move || {
            log_file
                .try_clone()
                .expect("failed to clone daemon log file handle")
        })
        .with_target(false)
        .json()
        .try_init()
        .ok();
    Ok(())
}

fn sleep_interruptible(seconds: u32, stop_requested: &Arc<AtomicBool>) {
    let slices = seconds.max(1) * 10;
    for _ in 0..slices {
        if stop_requested.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
