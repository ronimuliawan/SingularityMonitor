use anyhow::{Result, anyhow};
use std::mem::size_of;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

fn main() {
    if let Err(error) = run() {
        eprintln!("perf-harness failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let pid = parse_value(&args, "--pid")
        .ok_or_else(|| anyhow!("missing --pid <process-id> argument"))?
        .parse::<u32>()?;

    let samples = parse_value(&args, "--samples")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(12);
    let interval_ms = parse_value(&args, "--interval-ms")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(5_000);
    let max_bytes = parse_value(&args, "--max-bytes").and_then(|v| v.parse::<u64>().ok());

    let process = open_process(pid)?;
    println!("ts_unix,memory_bytes");

    let mut peak = 0u64;
    for _ in 0..samples {
        let memory = process_working_set_bytes(process)?;
        let ts = unix_timestamp();
        println!("{ts},{memory}");
        peak = peak.max(memory);
        thread::sleep(Duration::from_millis(interval_ms));
    }

    unsafe {
        CloseHandle(process);
    }

    if let Some(limit) = max_bytes
        && peak > limit
    {
        return Err(anyhow!(
            "memory limit breached: peak={} bytes, limit={} bytes",
            peak,
            limit
        ));
    }

    Ok(())
}

fn open_process(pid: u32) -> Result<HANDLE> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(anyhow!("OpenProcess failed for pid {pid}"));
    }

    Ok(process)
}

fn process_working_set_bytes(process: HANDLE) -> Result<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };

    let ok = unsafe {
        K32GetProcessMemoryInfo(
            process,
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };

    if ok == 0 {
        return Err(anyhow!("K32GetProcessMemoryInfo failed"));
    }

    Ok(counters.WorkingSetSize as u64)
}

fn parse_value(args: &[String], name: &str) -> Option<String> {
    let mut idx = 0usize;
    while idx < args.len() {
        if args[idx] == name {
            return args.get(idx + 1).cloned();
        }
        idx += 1;
    }
    None
}

fn unix_timestamp() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_secs()).unwrap_or(i64::MAX)
}
