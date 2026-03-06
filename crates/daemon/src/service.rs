use crate::config::{AppConfig, SERVICE_NAME};
use crate::db::Db;
use crate::runtime::{self, CollectorRuntime};
use crate::time::unix_timestamp;
use anyhow::Result;
use std::ffi::OsString;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tracing::warn;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

define_windows_service!(ffi_service_main, service_main);

pub fn run_service() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service_inner() {
        eprintln!("service runtime failure: {error:#}");
    }
}

fn run_service_inner() -> Result<()> {
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_handle = Arc::clone(&stop_requested);

    let status_handle =
        service_control_handler::register(SERVICE_NAME, move |event| match event {
            ServiceControl::Stop => {
                stop_handle.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;

    status_handle.set_service_status(service_status(ServiceState::StartPending))?;

    let config = AppConfig::load()?;
    runtime::init_logging(&config)?;
    let reliability_db = Db::initialize(&config.db_path)?;
    if let Err(error) = reliability_db.mark_daemon_start(unix_timestamp()) {
        warn!("failed to mark daemon service start: {error:#}");
    }
    let mut collector = match CollectorRuntime::new(config) {
        Ok(runtime) => runtime,
        Err(error) => {
            if let Err(record_error) = reliability_db.record_daemon_error(
                unix_timestamp(),
                "service_init",
                &error.to_string(),
            ) {
                warn!("failed to record daemon service init error: {record_error:#}");
            }
            return Err(error);
        }
    };

    status_handle.set_service_status(service_status(ServiceState::Running))?;
    let run_result = collector.run(stop_requested);
    status_handle.set_service_status(service_status(ServiceState::Stopped))?;

    match &run_result {
        Ok(()) => {
            if let Err(error) = reliability_db.mark_daemon_clean_exit(unix_timestamp()) {
                warn!("failed to mark daemon clean service exit: {error:#}");
            }
        }
        Err(error) => {
            if let Err(record_error) = reliability_db.record_daemon_error(
                unix_timestamp(),
                "service_run",
                &error.to_string(),
            ) {
                warn!("failed to record daemon service error: {record_error:#}");
            }
        }
    }

    run_result
}

fn service_status(state: ServiceState) -> ServiceStatus {
    let controls_accepted = if state == ServiceState::Running {
        ServiceControlAccept::STOP
    } else {
        ServiceControlAccept::empty()
    };

    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::from_secs(0),
        process_id: None,
    }
}
