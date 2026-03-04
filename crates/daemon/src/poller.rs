use crate::metered;
use anyhow::{anyhow, Result};
use std::ffi::c_void;
use tracing::warn;
use windows_sys::core::GUID;
use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

#[derive(Debug, Clone)]
pub struct InterfaceSnapshot {
    pub interface_guid: String,
    pub interface_name: String,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub interface_type: u32,
    pub is_metered: Option<bool>,
}

pub fn collect_interface_snapshot() -> Result<Vec<InterfaceSnapshot>> {
    let metered_by_guid = match metered::collect_interface_metered_map() {
        Ok(value) => value,
        Err(error) => {
            warn!("failed to query metered interface state: {error:#}");
            std::collections::HashMap::new()
        }
    };

    let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let status = unsafe { GetIfTable2(&mut table_ptr) };
    if status != 0 {
        return Err(anyhow!("GetIfTable2 failed with code {status}"));
    }

    if table_ptr.is_null() {
        return Err(anyhow!("GetIfTable2 returned null table"));
    }

    let entries = unsafe { (*table_ptr).NumEntries as usize };
    let first_row = unsafe { (*table_ptr).Table.as_ptr() };

    let mut rows = Vec::with_capacity(entries);
    for index in 0..entries {
        let row = unsafe { *first_row.add(index) };
        let alias = wide_to_string(&row.Alias);
        let description = wide_to_string(&row.Description);
        let interface_name = if alias.is_empty() { description } else { alias };
        let interface_guid = format_guid(row.InterfaceGuid);

        rows.push(InterfaceSnapshot {
            is_metered: metered_by_guid.get(&interface_guid).copied(),
            interface_guid,
            interface_name,
            bytes_sent: row.OutOctets,
            bytes_recv: row.InOctets,
            interface_type: row.Type,
        });
    }

    unsafe {
        FreeMibTable(table_ptr as *const c_void);
    }

    Ok(rows)
}

fn wide_to_string(chars: &[u16]) -> String {
    let end = chars.iter().position(|c| *c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end]).trim().to_string()
}

fn format_guid(guid: GUID) -> String {
    let d4 = guid.data4;
    format!(
        "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
        guid.data1, guid.data2, guid.data3, d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7]
    )
}
