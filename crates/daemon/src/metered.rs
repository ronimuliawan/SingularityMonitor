use anyhow::Result;
use std::collections::HashMap;
use windows::Networking::Connectivity::{ConnectionProfile, NetworkCostType, NetworkInformation};
use windows::core::GUID;

pub fn collect_interface_metered_map() -> Result<HashMap<String, bool>> {
    let mut metered_by_guid = HashMap::new();

    let profiles = NetworkInformation::GetConnectionProfiles()?;
    for index in 0..profiles.Size()? {
        let profile = profiles.GetAt(index)?;
        add_profile_metered_state(&profile, &mut metered_by_guid);
    }

    if let Ok(active_profile) = NetworkInformation::GetInternetConnectionProfile() {
        add_profile_metered_state(&active_profile, &mut metered_by_guid);
    }

    Ok(metered_by_guid)
}

fn add_profile_metered_state(
    profile: &ConnectionProfile,
    metered_by_guid: &mut HashMap<String, bool>,
) {
    let adapter = match profile.NetworkAdapter() {
        Ok(value) => value,
        Err(_) => return,
    };

    let adapter_guid = match adapter.NetworkAdapterId() {
        Ok(value) => value,
        Err(_) => return,
    };
    let cost = match profile.GetConnectionCost() {
        Ok(value) => value,
        Err(_) => return,
    };
    let cost_type = cost.NetworkCostType().unwrap_or(NetworkCostType::Unknown);
    let is_metered = matches!(
        cost_type,
        NetworkCostType::Fixed | NetworkCostType::Variable
    );

    metered_by_guid.insert(format_guid(adapter_guid), is_metered);
}

fn format_guid(guid: GUID) -> String {
    let d4 = guid.data4;
    format!(
        "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
        guid.data1, guid.data2, guid.data3, d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7]
    )
}
