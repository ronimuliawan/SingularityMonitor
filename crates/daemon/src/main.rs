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
use tracing::{error, info};

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

    let mut collector = runtime::CollectorRuntime::new(config)?;
    if run_once {
        collector.run_once()?;
        info!("completed single poll cycle");
        return Ok(());
    }

    info!("starting console collector loop");
    collector.run(stop_requested)
}
