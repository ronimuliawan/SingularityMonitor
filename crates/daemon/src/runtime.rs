use crate::config::AppConfig;
use crate::db::Db;
use crate::delta::DeltaEngine;
use crate::ipc::{self, IpcRequestHandler};
use crate::memory;
use crate::poller;
use crate::time::unix_timestamp;
use anyhow::Result;
use shared_contracts::{
    AppBreakdownRequest, DaemonStatusResponse, IngestAttributedUsageRequest,
    InterfaceBreakdownRequest, IpcMessage, SetImportStatusRequest, SetSettingsRequest,
    UsageSummaryRequest,
};
use std::fs::OpenOptions;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

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
            shared_contracts::METHOD_SET_SETTINGS => {
                let parsed = serde_json::from_value::<SetSettingsRequest>(request.payload.clone());
                match parsed {
                    Ok(payload) => {
                        let ts = unix_timestamp();
                        match self.db.apply_settings(ts, &payload) {
                            Ok(()) => {
                                if let Some(interval) = payload.poll_interval_seconds {
                                    if let Ok(mut state) = self.state.lock() {
                                        state.poll_interval_seconds = interval.clamp(15, 300);
                                    }
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
            shared_contracts::METHOD_GET_AFK_AUDIT => match self.db.query_afk_audit() {
                Ok(payload) => IpcMessage::response(&request, payload).unwrap_or_else(|error| {
                    IpcMessage::error_response(&request, 500, format!("encode failed: {error}"))
                }),
                Err(error) => IpcMessage::error_response(&request, 500, error.to_string()),
            },
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
    use super::resolve_observed_interval_secs;

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
}

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
