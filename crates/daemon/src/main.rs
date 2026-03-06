mod config;
mod db;
mod delta;
mod ipc;
mod memory;
mod metered;
mod poller;
mod runtime;
mod service;
mod time;

use anyhow::Result;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tracing::{error, info, warn};

fn main() {
    if let Err(error) = entrypoint() {
        eprintln!("daemon failed: {error:#}");
        std::process::exit(1);
    }
}

fn entrypoint() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let run_console = args.iter().any(|a| a == "--console");
    let run_once = args.iter().any(|a| a == "--once");
    let force_service = args.iter().any(|a| a == "--service");

    if run_console || run_once {
        let config = config::AppConfig::load()?;
        runtime::init_logging(&config)?;

        return run_console_mode(run_once, config);
    }

    if force_service {
        return service::run_service();
    }

    match service::run_service() {
        Ok(()) => Ok(()),
        Err(error) => {
            let config = config::AppConfig::load()?;
            runtime::init_logging(&config)?;
            error!(
                "service dispatch failed in interactive mode, falling back to console: {error:#}"
            );
            run_console_mode(false, config)
        }
    }
}

fn run_console_mode(run_once: bool, config: config::AppConfig) -> Result<()> {
    let stop_requested = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stop_requested);

    ctrlc::set_handler(move || {
        signal.store(true, Ordering::SeqCst);
    })?;

    let reliability_db = db::Db::initialize(&config.db_path)?;
    if let Err(error) = reliability_db.mark_daemon_start(time::unix_timestamp()) {
        warn!("failed to mark daemon start: {error:#}");
    }

    let run_result = (|| -> Result<()> {
        let mut collector = runtime::CollectorRuntime::new(config)?;
        if run_once {
            collector.run_once()?;
            info!("completed single poll cycle");
            return Ok(());
        }

        info!("starting console collector loop");
        collector.run(stop_requested)
    })();

    match run_result {
        Ok(()) => {
            if let Err(error) = reliability_db.mark_daemon_clean_exit(time::unix_timestamp()) {
                warn!("failed to mark daemon clean exit: {error:#}");
            }
            Ok(())
        }
        Err(error) => {
            let stage = if run_once {
                "console_run_once"
            } else {
                "console_run"
            };
            if let Err(record_error) = reliability_db.record_daemon_error(
                time::unix_timestamp(),
                stage,
                &error.to_string(),
            ) {
                warn!("failed to record daemon error: {record_error:#}");
            }
            Err(error)
        }
    }
}
