use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_timestamp() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_secs()).unwrap_or(i64::MAX)
}
