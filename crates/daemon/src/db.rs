use crate::delta::InterfaceDelta;
use crate::poller::InterfaceSnapshot;
use anyhow::{Context, Result};
use rusqlite::{named_params, params, Connection, OptionalExtension};
use shared_contracts::{
    AfkAuditResponse, AppBreakdownRequest, AppBreakdownResponse, AppUsageRow,
    GetInterfacesResponse, IngestAttributedUsageRequest, IngestAttributedUsageResponse,
    InterfaceBreakdownRequest, InterfaceBreakdownResponse, InterfaceInfo, InterfaceUsageRow,
    SetSettingsRequest, SettingsResponse, UsageBucket, UsageSummaryRequest, UsageSummaryResponse,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SYSTEM_PROCESS_NAME: &str = "System";
const ATTRIBUTION_INTERFACE_GUID: &str = "{11111111-1111-1111-1111-111111111111}";
const ATTRIBUTION_INTERFACE_NAME: &str = "Attributed Usage";

#[derive(Clone)]
pub struct Db {
    path: Arc<PathBuf>,
}

impl Db {
    pub fn initialize(path: impl AsRef<Path>) -> Result<Self> {
        let db = Self {
            path: Arc::new(path.as_ref().to_path_buf()),
        };
        let mut conn = db.open_connection()?;
        conn.execute_batch(SCHEMA_V1)?;
        db.ensure_default_settings(&mut conn)?;
        Ok(db)
    }

    pub fn get_poll_interval_seconds(&self, default: u32) -> Result<u32> {
        let conn = self.open_connection()?;
        let poll = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'poll_interval_seconds' LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|raw| raw.parse::<u32>().ok())
            .unwrap_or(default);

        Ok(poll.clamp(15, 300))
    }

    pub fn get_settings(&self) -> Result<SettingsResponse> {
        let conn = self.open_connection()?;

        let poll_interval_seconds = read_setting_u32(&conn, "poll_interval_seconds")?
            .unwrap_or(60)
            .clamp(15, 300);
        let retention_days = read_setting_u32(&conn, "retention_days")?
            .unwrap_or(0)
            .min(3650);
        let afk_idle_threshold_seconds = read_setting_u32(&conn, "afk_idle_threshold_seconds")?
            .unwrap_or(300)
            .clamp(30, 3600);

        Ok(SettingsResponse {
            poll_interval_seconds,
            retention_days,
            afk_idle_threshold_seconds,
        })
    }

    pub fn apply_settings(&self, ts: i64, settings: &SetSettingsRequest) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        if let Some(poll) = settings.poll_interval_seconds {
            let value = poll.clamp(15, 300).to_string();
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('poll_interval_seconds', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        if let Some(retention) = settings.retention_days {
            let value = retention.min(3650).to_string();
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('retention_days', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        if let Some(afk) = settings.afk_idle_threshold_seconds {
            let value = afk.clamp(30, 3600).to_string();
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('afk_idle_threshold_seconds', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn set_import_status(&self, ts: i64, status: &str, progress_pct: u8) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        upsert_setting(&tx, "import_status", status, ts)?;
        upsert_setting(&tx, "import_progress_pct", &progress_pct.to_string(), ts)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_import_status(&self) -> Result<(String, u8)> {
        let conn = self.open_connection()?;

        let status = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'import_status' LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| "idle".to_string());

        let progress = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'import_progress_pct' LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|raw| raw.parse::<u8>().ok())
            .unwrap_or(0)
            .min(100);

        Ok((status, progress))
    }

    pub fn insert_interface_deltas(&self, deltas: &[InterfaceDelta]) -> Result<()> {
        if deltas.is_empty() {
            return Ok(());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        let app_id = ensure_system_app(&tx)?;
        for delta in deltas {
            let interface_id = upsert_interface(
                &tx,
                &delta.interface_guid,
                &delta.interface_name,
                delta.interface_type,
                delta.is_metered,
                delta.ts,
            )?;

            tx.execute(
                "
                INSERT INTO usage_records(
                    ts,
                    app_id,
                    interface_id,
                    bytes_sent,
                    bytes_recv,
                    interval_secs,
                    source
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'interface_poll')
                ",
                params![
                    delta.ts,
                    app_id,
                    interface_id,
                    delta.bytes_sent as i64,
                    delta.bytes_recv as i64,
                    i64::from(delta.interval_secs),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn sync_interfaces(&self, snapshots: &[InterfaceSnapshot], ts: i64) -> Result<()> {
        if snapshots.is_empty() {
            return Ok(());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        for snapshot in snapshots {
            let _ = upsert_interface(
                &tx,
                &snapshot.interface_guid,
                &snapshot.interface_name,
                snapshot.interface_type,
                snapshot.is_metered,
                ts,
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn insert_attributed_usage(
        &self,
        payload: &IngestAttributedUsageRequest,
    ) -> Result<IngestAttributedUsageResponse> {
        if payload.samples.is_empty() {
            return Ok(IngestAttributedUsageResponse {
                accepted: 0,
                dropped: 0,
            });
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        let interface_name = payload
            .profile_name
            .as_deref()
            .map(|profile| format!("{} ({profile})", ATTRIBUTION_INTERFACE_NAME))
            .unwrap_or_else(|| ATTRIBUTION_INTERFACE_NAME.to_string());
        let interface_id = upsert_interface(
            &tx,
            ATTRIBUTION_INTERFACE_GUID,
            &interface_name,
            0,
            None,
            payload.end_ts,
        )?;

        let interval_secs = (payload.end_ts - payload.start_ts).max(1).clamp(1, 600) as u32;
        let source = match payload.source.as_deref() {
            Some("import") => "import",
            _ => "helper",
        };
        let mut accepted = 0u32;
        let mut dropped = 0u32;

        for sample in &payload.samples {
            if sample.bytes_sent == 0 && sample.bytes_recv == 0 {
                dropped = dropped.saturating_add(1);
                continue;
            }

            let process_name = normalize_process_name(&sample.attribution_id);
            let app_id = upsert_app(&tx, &process_name, payload.end_ts)?;

            tx.execute(
                "
                INSERT INTO usage_records(
                    ts,
                    app_id,
                    interface_id,
                    bytes_sent,
                    bytes_recv,
                    interval_secs,
                    source
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(ts, app_id, interface_id, source)
                DO UPDATE SET
                    bytes_sent = excluded.bytes_sent,
                    bytes_recv = excluded.bytes_recv,
                    interval_secs = excluded.interval_secs
                ",
                params![
                    payload.end_ts,
                    app_id,
                    interface_id,
                    sample.bytes_sent as i64,
                    sample.bytes_recv as i64,
                    i64::from(interval_secs),
                    source,
                ],
            )?;

            accepted = accepted.saturating_add(1);
        }

        tx.commit()?;

        Ok(IngestAttributedUsageResponse { accepted, dropped })
    }

    pub fn query_usage_summary(&self, req: &UsageSummaryRequest) -> Result<UsageSummaryResponse> {
        let conn = self.open_connection()?;
        let bucket_secs = granularity_to_seconds(&req.granularity);
        if let Some(app_filter) = req.app_filter.as_deref() {
            let mut stmt = conn.prepare(
                "
                WITH helper_cutover AS (
                    SELECT MIN(ts) AS ts
                    FROM usage_records
                    WHERE source = 'helper'
                )
                SELECT
                    (ur.ts / :bucket) * :bucket AS bucket_ts,
                    SUM(ur.bytes_sent) AS sent,
                    SUM(ur.bytes_recv) AS recv
                FROM usage_records ur
                JOIN interfaces i ON i.id = ur.interface_id
                JOIN apps a ON a.id = ur.app_id
                CROSS JOIN helper_cutover hc
                WHERE ur.ts >= :start_ts
                  AND ur.ts < :end_ts
                  AND a.process_name = :app_filter
                  AND (:interface_id IS NULL OR i.guid = :interface_id)
                  AND (:interface_type IS NULL OR i.type = :interface_type)
                  AND (
                      ur.source = 'helper'
                      OR (ur.source = 'import' AND (hc.ts IS NULL OR ur.ts < hc.ts))
                  )
                GROUP BY bucket_ts
                ORDER BY bucket_ts ASC
                ",
            )?;

            let rows = stmt.query_map(
                named_params! {
                    ":bucket": bucket_secs,
                    ":start_ts": req.start_ts,
                    ":end_ts": req.end_ts,
                    ":app_filter": app_filter,
                    ":interface_id": req.interface_id.as_deref(),
                    ":interface_type": req.interface_type.as_deref(),
                },
                |row| {
                    Ok(UsageBucket {
                        ts: row.get::<_, i64>(0)?,
                        bytes_sent: row.get::<_, i64>(1)?.max(0) as u64,
                        bytes_recv: row.get::<_, i64>(2)?.max(0) as u64,
                        interface_id: req.interface_id.clone(),
                    })
                },
            )?;

            let mut buckets = Vec::new();
            let mut total_sent = 0u64;
            let mut total_recv = 0u64;
            for row in rows {
                let bucket = row?;
                total_sent = total_sent.saturating_add(bucket.bytes_sent);
                total_recv = total_recv.saturating_add(bucket.bytes_recv);
                buckets.push(bucket);
            }

            return Ok(UsageSummaryResponse {
                buckets,
                total_sent,
                total_recv,
            });
        }

        let mut stmt = conn.prepare(
            "
            WITH poll_cutover AS (
                SELECT MIN(candidate_ts) AS ts
                FROM (
                    SELECT MIN(ts) AS candidate_ts
                    FROM usage_records
                    WHERE source IN ('interface_poll', 'poll')

                    UNION ALL

                    SELECT MIN(last_seen) AS candidate_ts
                    FROM interfaces
                    WHERE guid <> '{11111111-1111-1111-1111-111111111111}'
                ) cutover_candidates
                WHERE candidate_ts IS NOT NULL
            )
            SELECT
                (ur.ts / :bucket) * :bucket AS bucket_ts,
                SUM(ur.bytes_sent) AS sent,
                SUM(ur.bytes_recv) AS recv
            FROM usage_records ur
            JOIN interfaces i ON i.id = ur.interface_id
            CROSS JOIN poll_cutover pc
            WHERE ur.ts >= :start_ts
              AND ur.ts < :end_ts
              AND (:interface_id IS NULL OR i.guid = :interface_id)
              AND (:interface_type IS NULL OR i.type = :interface_type)
              AND (
                  ur.source IN ('interface_poll', 'poll')
                  OR (ur.source = 'import' AND (pc.ts IS NULL OR ur.ts < pc.ts))
              )
            GROUP BY bucket_ts
            ORDER BY bucket_ts ASC
            ",
        )?;

        let rows = stmt.query_map(
            named_params! {
                ":bucket": bucket_secs,
                ":start_ts": req.start_ts,
                ":end_ts": req.end_ts,
                ":interface_id": req.interface_id.as_deref(),
                ":interface_type": req.interface_type.as_deref(),
            },
            |row| {
                Ok(UsageBucket {
                    ts: row.get::<_, i64>(0)?,
                    bytes_sent: row.get::<_, i64>(1)?.max(0) as u64,
                    bytes_recv: row.get::<_, i64>(2)?.max(0) as u64,
                    interface_id: req.interface_id.clone(),
                })
            },
        )?;

        let mut buckets = Vec::new();
        let mut total_sent = 0u64;
        let mut total_recv = 0u64;
        for row in rows {
            let bucket = row?;
            total_sent = total_sent.saturating_add(bucket.bytes_sent);
            total_recv = total_recv.saturating_add(bucket.bytes_recv);
            buckets.push(bucket);
        }

        Ok(UsageSummaryResponse {
            buckets,
            total_sent,
            total_recv,
        })
    }

    pub fn query_app_breakdown(&self, req: &AppBreakdownRequest) -> Result<AppBreakdownResponse> {
        let conn = self.open_connection()?;
        let limit = req.limit.unwrap_or(50).min(500);
        let mut stmt = conn.prepare(
            "
            WITH helper_cutover AS (
                SELECT MIN(ts) AS ts
                FROM usage_records
                WHERE source = 'helper'
            )
            SELECT
                a.process_name,
                COALESCE(a.display_name, a.process_name) AS display_name,
                SUM(ur.bytes_sent) AS sent,
                SUM(ur.bytes_recv) AS recv,
                MAX(ur.ts) AS last_seen
            FROM usage_records ur
            JOIN apps a ON a.id = ur.app_id
            JOIN interfaces i ON i.id = ur.interface_id
            CROSS JOIN helper_cutover hc
            WHERE ur.ts >= :start_ts
              AND ur.ts < :end_ts
              AND (:interface_id IS NULL OR i.guid = :interface_id)
              AND (:interface_type IS NULL OR i.type = :interface_type)
              AND (
                  ur.source = 'helper'
                  OR (ur.source = 'import' AND (hc.ts IS NULL OR ur.ts < hc.ts))
              )
            GROUP BY a.id, a.process_name, display_name
            ORDER BY (sent + recv) DESC
            LIMIT :limit
            ",
        )?;

        let rows = stmt.query_map(
            named_params! {
                ":start_ts": req.start_ts,
                ":end_ts": req.end_ts,
                ":interface_id": req.interface_id.as_deref(),
                ":interface_type": req.interface_type.as_deref(),
                ":limit": i64::from(limit),
            },
            |row| {
                Ok(AppUsageRow {
                    process_name: row.get(0)?,
                    display_name: row.get(1)?,
                    bytes_sent: row.get::<_, i64>(2)?.max(0) as u64,
                    bytes_recv: row.get::<_, i64>(3)?.max(0) as u64,
                    last_seen_ts: row.get::<_, i64>(4)?,
                })
            },
        )?;

        let apps = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(AppBreakdownResponse {
            total_apps: apps.len() as u32,
            apps,
        })
    }

    pub fn query_interfaces(&self) -> Result<GetInterfacesResponse> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "
            SELECT guid, name, type, is_metered
            FROM interfaces
            ORDER BY name COLLATE NOCASE ASC
            ",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(InterfaceInfo {
                guid: row.get::<_, String>(0)?,
                name: row.get::<_, String>(1)?,
                interface_type: row.get::<_, String>(2)?,
                is_metered: row.get::<_, i64>(3)? != 0,
            })
        })?;

        let interfaces = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(GetInterfacesResponse { interfaces })
    }

    pub fn query_interface_breakdown(
        &self,
        req: &InterfaceBreakdownRequest,
    ) -> Result<InterfaceBreakdownResponse> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "
            SELECT
                i.guid,
                i.name,
                i.type,
                i.is_metered,
                SUM(ur.bytes_sent) AS sent,
                SUM(ur.bytes_recv) AS recv
            FROM usage_records ur
            JOIN interfaces i ON i.id = ur.interface_id
            WHERE ur.ts >= :start_ts
              AND ur.ts < :end_ts
              AND ur.source IN ('interface_poll', 'poll')
              AND (:interface_id IS NULL OR i.guid = :interface_id)
              AND (:interface_type IS NULL OR i.type = :interface_type)
            GROUP BY i.id, i.guid, i.name, i.type, i.is_metered
            ORDER BY (sent + recv) DESC
            ",
        )?;

        let rows = stmt.query_map(
            named_params! {
                ":start_ts": req.start_ts,
                ":end_ts": req.end_ts,
                ":interface_id": req.interface_id,
                ":interface_type": req.interface_type,
            },
            |row| {
                Ok(InterfaceUsageRow {
                    interface_id: row.get::<_, String>(0)?,
                    interface_name: row.get::<_, String>(1)?,
                    interface_type: row.get::<_, String>(2)?,
                    is_metered: row.get::<_, i64>(3)? != 0,
                    bytes_sent: row.get::<_, i64>(4)?.max(0) as u64,
                    bytes_recv: row.get::<_, i64>(5)?.max(0) as u64,
                })
            },
        )?;

        let interfaces = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(InterfaceBreakdownResponse {
            total_interfaces: interfaces.len() as u32,
            interfaces,
        })
    }

    pub fn query_afk_audit(&self) -> Result<AfkAuditResponse> {
        Ok(AfkAuditResponse {
            afk_windows: Vec::new(),
        })
    }

    pub fn db_size_bytes(&self) -> u64 {
        std::fs::metadata(self.path.as_ref())
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn last_helper_ingest_ts(&self) -> Result<i64> {
        let conn = self.open_connection()?;
        let ts = conn.query_row(
            "
            SELECT COALESCE(MAX(ts), 0)
            FROM usage_records
            WHERE source IN ('helper', 'import')
            ",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(ts)
    }

    fn open_connection(&self) -> Result<Connection> {
        let conn = Connection::open(self.path.as_ref())
            .with_context(|| format!("failed to open sqlite at {}", self.path.display()))?;

        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = MEMORY;
            PRAGMA cache_size = -512;
            PRAGMA wal_autocheckpoint = 500;
            ",
        )?;

        Ok(conn)
    }

    fn ensure_default_settings(&self, conn: &mut Connection) -> Result<()> {
        let tx = conn.transaction()?;
        upsert_setting(&tx, "poll_interval_seconds", "60", 0)?;
        upsert_setting(&tx, "retention_days", "0", 0)?;
        upsert_setting(&tx, "afk_idle_threshold_seconds", "300", 0)?;
        upsert_setting(&tx, "import_status", "idle", 0)?;
        upsert_setting(&tx, "import_progress_pct", "0", 0)?;
        tx.commit()?;
        Ok(())
    }
}

fn granularity_to_seconds(granularity: &str) -> i64 {
    match granularity {
        "hour" => 3600,
        "week" => 7 * 24 * 3600,
        "month" => 30 * 24 * 3600,
        _ => 24 * 3600,
    }
}

fn ensure_system_app(conn: &Connection) -> Result<i64> {
    upsert_app(conn, SYSTEM_PROCESS_NAME, current_unix_ts())
}

fn upsert_interface(
    conn: &Connection,
    guid: &str,
    name: &str,
    interface_type: u32,
    is_metered: Option<bool>,
    ts: i64,
) -> Result<i64> {
    let kind = match interface_type {
        6 => "ethernet",
        71 => "wifi",
        24 => "loopback",
        _ => "other",
    };

    conn.execute(
        "
        INSERT INTO interfaces(guid, name, type, is_metered, first_seen, last_seen)
        VALUES(?1, ?2, ?3, COALESCE(?4, 0), ?5, ?5)
        ON CONFLICT(guid)
        DO UPDATE SET
            name = excluded.name,
            type = excluded.type,
            is_metered = CASE
                WHEN ?4 IS NULL THEN interfaces.is_metered
                ELSE excluded.is_metered
            END,
            last_seen = excluded.last_seen
        ",
        params![
            guid,
            name,
            kind,
            is_metered.map(|flag| if flag { 1_i64 } else { 0_i64 }),
            ts
        ],
    )?;

    let interface_id = conn.query_row(
        "SELECT id FROM interfaces WHERE guid = ?1",
        params![guid],
        |row| row.get(0),
    )?;
    Ok(interface_id)
}

fn upsert_app(conn: &Connection, process_name: &str, ts: i64) -> Result<i64> {
    conn.execute(
        "
        INSERT INTO apps(process_name, display_name, first_seen, last_seen)
        VALUES(?1, ?2, ?3, ?3)
        ON CONFLICT(process_name)
        DO UPDATE SET
            display_name = excluded.display_name,
            last_seen = excluded.last_seen
        ",
        params![process_name, process_name, ts],
    )?;

    let app_id = conn.query_row(
        "SELECT id FROM apps WHERE process_name = ?1",
        params![process_name],
        |row| row.get(0),
    )?;
    Ok(app_id)
}

fn normalize_process_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return "unattributed".to_string();
    }

    let lowered = trimmed.to_ascii_lowercase();
    if lowered.ends_with(".exe") || lowered.ends_with(".com") || lowered.ends_with(".bat") {
        return trimmed
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(trimmed)
            .to_string();
    }

    if let Some(file_name) = trimmed.rsplit(['\\', '/']).next() {
        if file_name.to_ascii_lowercase().ends_with(".exe") {
            return file_name.to_string();
        }
    }

    trimmed.to_string()
}

fn current_unix_ts() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_secs()).unwrap_or(i64::MAX)
}

fn upsert_setting(conn: &Connection, key: &str, value: &str, ts: i64) -> Result<()> {
    conn.execute(
        "
        INSERT INTO settings(key, value, updated_at)
        VALUES(?1, ?2, ?3)
        ON CONFLICT(key)
        DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
        ",
        params![key, value, ts],
    )?;
    Ok(())
}

fn read_setting_u32(conn: &Connection, key: &str) -> Result<Option<u32>> {
    let raw = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(raw.and_then(|value| value.parse::<u32>().ok()))
}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS interfaces (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    guid        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    type        TEXT NOT NULL,
    is_metered  INTEGER NOT NULL DEFAULT 0,
    first_seen  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_interfaces_guid ON interfaces(guid);

CREATE TABLE IF NOT EXISTS apps (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    process_name TEXT NOT NULL UNIQUE,
    display_name TEXT,
    first_seen   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_apps_process_name ON apps(process_name);

CREATE TABLE IF NOT EXISTS usage_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts              INTEGER NOT NULL,
    app_id          INTEGER NOT NULL REFERENCES apps(id),
    interface_id    INTEGER NOT NULL REFERENCES interfaces(id),
    bytes_sent      INTEGER NOT NULL DEFAULT 0,
    bytes_recv      INTEGER NOT NULL DEFAULT 0,
    interval_secs   INTEGER NOT NULL DEFAULT 60,
    source          TEXT NOT NULL DEFAULT 'poll'
);
DELETE FROM usage_records
WHERE id NOT IN (
    SELECT MIN(id)
    FROM usage_records
    GROUP BY ts, app_id, interface_id, source
);
CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_records(ts);
CREATE INDEX IF NOT EXISTS idx_usage_app_ts ON usage_records(app_id, ts);
CREATE INDEX IF NOT EXISTS idx_usage_iface_ts ON usage_records(interface_id, ts);
CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_unique
    ON usage_records(ts, app_id, interface_id, source);

CREATE TABLE IF NOT EXISTS afk_windows (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    start_ts    INTEGER NOT NULL,
    end_ts      INTEGER NOT NULL,
    source      TEXT NOT NULL DEFAULT 'wts'
);
CREATE INDEX IF NOT EXISTS idx_afk_start ON afk_windows(start_ts);
CREATE INDEX IF NOT EXISTS idx_afk_end ON afk_windows(end_ts);

CREATE TABLE IF NOT EXISTS alert_definitions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    alert_type      TEXT NOT NULL,
    interface_id    INTEGER REFERENCES interfaces(id),
    threshold_value REAL NOT NULL,
    cap_bytes       INTEGER,
    is_active       INTEGER NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS alert_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    definition_id   INTEGER NOT NULL REFERENCES alert_definitions(id),
    fired_at        INTEGER NOT NULL,
    current_bytes   INTEGER NOT NULL,
    message         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_alert_events_ts ON alert_events(fired_at);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS import_log (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at       INTEGER NOT NULL,
    completed_at     INTEGER,
    periods_imported INTEGER,
    status           TEXT NOT NULL
);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_ATTRIBUTION_GUID: &str = "{11111111-1111-1111-1111-111111111111}";
    const TEST_ETHERNET_GUID: &str = "{22222222-2222-2222-2222-222222222222}";

    fn new_test_db() -> Db {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!(
            "singularity-monitor-db-test-{}-{}.db",
            std::process::id(),
            nonce
        ));
        let _ = std::fs::remove_file(&path);
        Db::initialize(path).expect("failed to initialize test db")
    }

    fn insert_usage(
        db: &Db,
        ts: i64,
        process_name: &str,
        interface_guid: &str,
        interface_name: &str,
        interface_type: u32,
        bytes_sent: u64,
        bytes_recv: u64,
        source: &str,
    ) {
        let mut conn = db.open_connection().expect("open db");
        let tx = conn.transaction().expect("begin tx");
        let interface_id = upsert_interface(
            &tx,
            interface_guid,
            interface_name,
            interface_type,
            Some(false),
            ts,
        )
        .expect("upsert interface");
        let app_id = upsert_app(&tx, process_name, ts).expect("upsert app");

        tx.execute(
            "
            INSERT INTO usage_records(
                ts,
                app_id,
                interface_id,
                bytes_sent,
                bytes_recv,
                interval_secs,
                source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                ts,
                app_id,
                interface_id,
                bytes_sent as i64,
                bytes_recv as i64,
                60i64,
                source,
            ],
        )
        .expect("insert usage");
        tx.commit().expect("commit tx");
    }

    #[test]
    fn app_breakdown_prefers_helper_after_cutover() {
        let db = new_test_db();
        let app = "sm_overlap_case.exe";

        insert_usage(
            &db,
            100,
            app,
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            100,
            50,
            "import",
        );
        insert_usage(
            &db,
            300,
            app,
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            400,
            100,
            "import",
        );
        insert_usage(
            &db,
            300,
            app,
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            40,
            10,
            "helper",
        );

        let response = db
            .query_app_breakdown(&AppBreakdownRequest {
                start_ts: 0,
                end_ts: 1000,
                interface_id: None,
                interface_type: None,
                limit: Some(10),
                sort_by: Some("total_bytes_desc".to_string()),
            })
            .expect("query app breakdown");

        let app_row = response
            .apps
            .iter()
            .find(|row| row.process_name == app)
            .expect("missing app row");

        assert_eq!(app_row.bytes_sent, 140);
        assert_eq!(app_row.bytes_recv, 60);
    }

    #[test]
    fn usage_summary_with_app_filter_prefers_helper_after_cutover() {
        let db = new_test_db();
        let app = "sm_summary_case.exe";

        insert_usage(
            &db,
            100,
            app,
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            200,
            100,
            "import",
        );
        insert_usage(
            &db,
            300,
            app,
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            600,
            200,
            "import",
        );
        insert_usage(
            &db,
            300,
            app,
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            60,
            20,
            "helper",
        );

        let response = db
            .query_usage_summary(&UsageSummaryRequest {
                start_ts: 0,
                end_ts: 1000,
                granularity: "hour".to_string(),
                interface_id: Some(TEST_ATTRIBUTION_GUID.to_string()),
                interface_type: None,
                app_filter: Some(app.to_string()),
            })
            .expect("query usage summary");

        assert_eq!(response.total_sent, 260);
        assert_eq!(response.total_recv, 120);
    }

    #[test]
    fn usage_summary_without_app_filter_prefers_poll_after_cutover() {
        let db = new_test_db();

        insert_usage(
            &db,
            100,
            "legacy-import.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            80,
            20,
            "import",
        );
        insert_usage(
            &db,
            300,
            "overlap-import.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            500,
            300,
            "import",
        );
        insert_usage(
            &db,
            250,
            "System",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            1000,
            1000,
            "interface_poll",
        );

        let response = db
            .query_usage_summary(&UsageSummaryRequest {
                start_ts: 0,
                end_ts: 1000,
                granularity: "hour".to_string(),
                interface_id: Some(TEST_ATTRIBUTION_GUID.to_string()),
                interface_type: None,
                app_filter: None,
            })
            .expect("query usage summary");

        assert_eq!(response.total_sent, 80);
        assert_eq!(response.total_recv, 20);
    }
}
