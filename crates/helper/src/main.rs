use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use shared_contracts::{
    AttributedUsageSample, IngestAttributedUsageRequest, IngestAttributedUsageResponse, IpcMessage,
    MessageType, SetImportStatusRequest,
};
use std::fs::{OpenOptions, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use windows::Foundation::DateTime;
use windows::Networking::Connectivity::{
    DataUsageGranularity, NetworkInformation, NetworkUsageStates,
};

const SECS_BETWEEN_EPOCHS: i64 = 11_644_473_600;
const TICKS_PER_SECOND: i64 = 10_000_000;
const DEFAULT_WINDOW_SECS: u32 = 300;
const DEFAULT_LOOP_INTERVAL_SECS: u32 = 60;
const DEFAULT_IMPORT_DAYS: u32 = 60;
const DEFAULT_IMPORT_CHUNK_HOURS: u32 = 6;
const DAEMON_PIPE_PATH: &str = r"\\.\pipe\SingularityMonitor";

#[derive(Debug, Serialize)]
struct AttributedSample {
    attribution_id: String,
    bytes_sent: u64,
    bytes_recv: u64,
}

#[derive(Debug, Serialize)]
struct ProbeResult {
    start_ts: i64,
    end_ts: i64,
    aggregate_sent: u64,
    aggregate_recv: u64,
    profile_name: Option<String>,
    attributed: Vec<AttributedSample>,
}

enum Mode {
    Probe {
        window_secs: u32,
    },
    PushOnce {
        window_secs: u32,
    },
    Loop {
        window_secs: u32,
        interval_secs: u32,
    },
    ImportHistory {
        days: u32,
        chunk_hours: u32,
    },
}

fn main() {
    append_reliability_event("start", "main", "helper process initialized");
    match run() {
        Ok(()) => {
            append_reliability_event("clean_exit", "main", "helper exited cleanly");
        }
        Err(error) => {
            append_reliability_event("error", "main", &format!("{error:#}"));
            eprintln!("helper failed: {error:#}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mode = parse_mode(&args)?;

    match mode {
        Mode::Probe { window_secs } => {
            let result = collect_snapshot(window_secs)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Mode::PushOnce { window_secs } => {
            let result = collect_snapshot(window_secs)?;
            let ingest = push_to_daemon(&result, "helper")?;
            println!(
                "ingest accepted={} dropped={}",
                ingest.accepted, ingest.dropped
            );
        }
        Mode::Loop {
            window_secs,
            interval_secs,
        } => loop {
            match collect_snapshot(window_secs)
                .and_then(|snapshot| push_to_daemon(&snapshot, "helper"))
            {
                Ok(ingest) => {
                    println!(
                        "ingest accepted={} dropped={}",
                        ingest.accepted, ingest.dropped
                    )
                }
                Err(error) => eprintln!("loop ingest failed: {error:#}"),
            }
            thread::sleep(Duration::from_secs(interval_secs as u64));
        },
        Mode::ImportHistory { days, chunk_hours } => {
            run_history_import(days, chunk_hours)?;
        }
    }

    Ok(())
}

fn run_history_import(days: u32, chunk_hours: u32) -> Result<()> {
    let end_ts = unix_timestamp();
    let start_ts = end_ts.saturating_sub(i64::from(days.max(1)) * 86_400);
    let chunk_secs = i64::from(chunk_hours.max(1)) * 3_600;

    let mut cursor = start_ts;
    let mut chunk_index = 0u32;
    let mut accepted_total = 0u64;
    let mut dropped_total = 0u64;

    send_import_status("running", 0).ok();
    println!("starting import: days={days}, chunk_hours={chunk_hours}");

    while cursor < end_ts {
        let chunk_end = (cursor + chunk_secs).min(end_ts);
        let snapshot = collect_range(cursor, chunk_end)?;
        let ingest = push_to_daemon(&snapshot, "import")?;

        accepted_total = accepted_total.saturating_add(u64::from(ingest.accepted));
        dropped_total = dropped_total.saturating_add(u64::from(ingest.dropped));
        chunk_index = chunk_index.saturating_add(1);

        let progress = ((((chunk_end - start_ts) as f64) / ((end_ts - start_ts).max(1) as f64))
            * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        send_import_status("running", progress).ok();

        println!(
            "import chunk={} range=[{},{}] accepted={} dropped={} progress={}%%",
            chunk_index, cursor, chunk_end, ingest.accepted, ingest.dropped, progress
        );
        cursor = chunk_end;
    }

    send_import_status("complete", 100).ok();
    println!(
        "import complete: chunks={} accepted_total={} dropped_total={}",
        chunk_index, accepted_total, dropped_total
    );

    Ok(())
}

fn collect_snapshot(window_secs: u32) -> Result<ProbeResult> {
    let end_ts = unix_timestamp();
    let start_ts = end_ts.saturating_sub(i64::from(window_secs.max(1)));
    collect_range(start_ts, end_ts)
}

fn collect_range(start_ts: i64, end_ts: i64) -> Result<ProbeResult> {
    let profile = NetworkInformation::GetInternetConnectionProfile()?;
    let profile_name = profile
        .ProfileName()
        .ok()
        .map(|name| name.to_string())
        .filter(|name| !name.is_empty());

    let start = unix_to_winrt(start_ts);
    let end = unix_to_winrt(end_ts);

    let aggregate_usage = profile
        .GetNetworkUsageAsync(
            start,
            end,
            DataUsageGranularity::PerHour,
            NetworkUsageStates::default(),
        )?
        .join()?;

    let mut aggregate_sent = 0u64;
    let mut aggregate_recv = 0u64;
    for index in 0..aggregate_usage.Size()? {
        let usage = aggregate_usage.GetAt(index)?;
        aggregate_sent = aggregate_sent.saturating_add(usage.BytesSent()?);
        aggregate_recv = aggregate_recv.saturating_add(usage.BytesReceived()?);
    }

    let attributed_usage = profile
        .GetAttributedNetworkUsageAsync(start, end, NetworkUsageStates::default())?
        .join()?;

    let mut attributed = Vec::new();
    for index in 0..attributed_usage.Size()? {
        let item = attributed_usage.GetAt(index)?;
        let attribution_id = item
            .AttributionId()
            .ok()
            .map(|value| normalize_attribution_id(&format!("{value:?}")))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "unattributed".to_string());

        attributed.push(AttributedSample {
            attribution_id,
            bytes_sent: item.BytesSent()?,
            bytes_recv: item.BytesReceived()?,
        });
    }

    Ok(ProbeResult {
        start_ts,
        end_ts,
        aggregate_sent,
        aggregate_recv,
        profile_name,
        attributed,
    })
}

fn push_to_daemon(snapshot: &ProbeResult, source: &str) -> Result<IngestAttributedUsageResponse> {
    let payload = IngestAttributedUsageRequest {
        start_ts: snapshot.start_ts,
        end_ts: snapshot.end_ts,
        profile_name: snapshot.profile_name.clone(),
        source: Some(source.to_string()),
        aggregate_sent: Some(snapshot.aggregate_sent),
        aggregate_recv: Some(snapshot.aggregate_recv),
        samples: snapshot
            .attributed
            .iter()
            .map(|sample| AttributedUsageSample {
                attribution_id: sample.attribution_id.clone(),
                bytes_sent: sample.bytes_sent,
                bytes_recv: sample.bytes_recv,
            })
            .collect(),
    };

    let request = IpcMessage::request(shared_contracts::METHOD_INGEST_ATTRIBUTED_USAGE, payload)?;
    let response = send_daemon_request(&request)?;

    if let Some(error) = response.error {
        return Err(anyhow!("daemon returned {}: {}", error.code, error.message));
    }

    let ingest = serde_json::from_value::<IngestAttributedUsageResponse>(response.payload)
        .context("daemon response payload did not match ingest response")?;
    Ok(ingest)
}

fn send_import_status(status: &str, progress_pct: u8) -> Result<()> {
    let payload = SetImportStatusRequest {
        status: status.to_string(),
        progress_pct: progress_pct.min(100),
    };

    let request = IpcMessage::request(shared_contracts::METHOD_SET_IMPORT_STATUS, payload)?;
    let response = send_daemon_request(&request)?;
    if let Some(error) = response.error {
        return Err(anyhow!("daemon returned {}: {}", error.code, error.message));
    }

    Ok(())
}

fn send_daemon_request(request: &IpcMessage) -> Result<IpcMessage> {
    let mut stream = OpenOptions::new()
        .read(true)
        .write(true)
        .open(DAEMON_PIPE_PATH)
        .with_context(|| format!("failed to connect to daemon pipe {DAEMON_PIPE_PATH}"))?;

    let request_line = request.to_line()?;
    stream.write_all(request_line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    let bytes_read = reader.read_line(&mut response_line)?;
    if bytes_read == 0 || response_line.trim().is_empty() {
        return Err(anyhow!("daemon returned an empty response"));
    }

    let response = IpcMessage::from_line(response_line.trim_end())?;
    if response.message_type != MessageType::Response {
        return Err(anyhow!("daemon returned non-response message type"));
    }
    if response.method != request.method {
        return Err(anyhow!(
            "daemon response method mismatch: expected {}, got {}",
            request.method,
            response.method
        ));
    }

    Ok(response)
}

fn normalize_attribution_id(raw: &str) -> String {
    raw.trim().trim_matches('"').to_string()
}

fn parse_mode(args: &[String]) -> Result<Mode> {
    if args.iter().any(|arg| arg == "--import-history") {
        let days = parse_u32_flag(args, "--days")
            .unwrap_or(DEFAULT_IMPORT_DAYS)
            .max(1);
        let chunk_hours = parse_u32_flag(args, "--chunk-hours")
            .unwrap_or(DEFAULT_IMPORT_CHUNK_HOURS)
            .max(1);
        return Ok(Mode::ImportHistory { days, chunk_hours });
    }

    let window_secs = parse_u32_flag(args, "--window-secs").unwrap_or(DEFAULT_WINDOW_SECS);
    if args.iter().any(|arg| arg == "--push-once") {
        return Ok(Mode::PushOnce { window_secs });
    }

    if args.iter().any(|arg| arg == "--loop") {
        let interval_secs = parse_u32_flag(args, "--interval-secs")
            .unwrap_or(DEFAULT_LOOP_INTERVAL_SECS)
            .max(15);
        return Ok(Mode::Loop {
            window_secs,
            interval_secs,
        });
    }

    if args.iter().any(|arg| arg == "--probe") || args.is_empty() {
        return Ok(Mode::Probe { window_secs });
    }

    Err(anyhow!(
        "unsupported helper mode. use --probe, --push-once, --loop, or --import-history"
    ))
}

fn parse_u32_flag(args: &[String], flag: &str) -> Option<u32> {
    args.windows(2)
        .find_map(|pair| (pair[0] == flag).then(|| pair[1].parse::<u32>().ok()))
        .flatten()
}

fn unix_timestamp() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_secs()).unwrap_or(i64::MAX)
}

fn append_reliability_event(kind: &str, stage: &str, message: &str) {
    let path = resolve_reliability_log_path();
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }

    let payload = serde_json::json!({
        "ts": unix_timestamp(),
        "kind": kind,
        "stage": stage,
        "message": message,
    });

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", payload);
    }
}

fn resolve_reliability_log_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("SM_DATA_ROOT") {
        return PathBuf::from(explicit).join("helper-reliability.jsonl");
    }

    if let Some(program_data) = std::env::var_os("ProgramData") {
        return PathBuf::from(program_data)
            .join("SingularityMonitor")
            .join("helper-reliability.jsonl");
    }

    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SingularityMonitorDev")
        .join("helper-reliability.jsonl")
}

fn unix_to_winrt(unix_ts: i64) -> DateTime {
    DateTime {
        UniversalTime: (unix_ts + SECS_BETWEEN_EPOCHS) * TICKS_PER_SECOND,
    }
}
