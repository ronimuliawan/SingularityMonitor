use anyhow::{Result, anyhow};
use std::mem::size_of;
use windows_sys::Win32::System::ProcessStatus::{
    EmptyWorkingSet, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub fn current_working_set_bytes() -> Result<u64> {
    let process = unsafe { GetCurrentProcess() };
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

pub fn trim_working_set() -> Result<()> {
    let process = unsafe { GetCurrentProcess() };
    let ok = unsafe { EmptyWorkingSet(process) };
    if ok == 0 {
        return Err(anyhow!("EmptyWorkingSet failed"));
    }

    Ok(())
}
