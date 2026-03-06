use crate::delta::InterfaceDelta;
use crate::poller::InterfaceSnapshot;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, named_params, params};
use shared_contracts::{
    AfkAuditResponse, AfkWindowUsage, AppBreakdownRequest, AppBreakdownResponse, AppUsageRow,
    CapAlertEvent, CapDefinition, CompactDatabaseResponse, DeleteCapDefinitionRequest,
    DeleteCapDefinitionResponse, GetAfkAuditRequest, GetInterfacesResponse,
    IngestAttributedUsageRequest, IngestAttributedUsageResponse, InterfaceBreakdownRequest,
    InterfaceBreakdownResponse, InterfaceInfo, InterfaceUsageRow, ListCapAlertEventsRequest,
    ListCapAlertEventsResponse, ListCapDefinitionsResponse, MarkCapAlertEventsDeliveredResponse,
    SetSettingsRequest, SettingsResponse, UpsertCapDefinitionRequest, UpsertCapDefinitionResponse,
    UsageBucket, UsageSummaryRequest, UsageSummaryResponse,
};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const SYSTEM_PROCESS_NAME: &str = "System";
const ATTRIBUTION_INTERFACE_GUID: &str = "{11111111-1111-1111-1111-111111111111}";
const ATTRIBUTION_INTERFACE_NAME: &str = "Attributed Usage";
const CAP_SCOPE_GLOBAL: &str = "global";
const CAP_SCOPE_INTERFACE: &str = "interface";
const CAP_ALERT_WINDOW_MONTHLY: &str = "monthly";
const CAP_ALERT_WINDOW_DAILY: &str = "daily";
const CAP_ALERT_DELIVERY_NEW: &str = "new";
const CAP_ALERT_THRESHOLDS_PCT: [u64; 3] = [50, 80, 95];
const RETENTION_KEY_LAST_RUN_TS: &str = "retention_cleanup_last_run_ts";
const RETENTION_KEY_LAST_RUN_DAY_START_TS: &str = "retention_cleanup_last_run_day_start_ts";
const RETENTION_KEY_CUTOFF_TS: &str = "retention_cleanup_cutoff_ts";
const RETENTION_KEY_DELETED_USAGE_RECORDS: &str = "retention_cleanup_deleted_usage_records";
const RETENTION_KEY_DELETED_AFK_WINDOWS: &str = "retention_cleanup_deleted_afk_windows";
const RETENTION_KEY_LAST_RESULT: &str = "retention_cleanup_last_result";
const RELIABILITY_KEY_SESSION_OPEN: &str = "daemon_reliability_session_open";
const RELIABILITY_KEY_START_COUNT: &str = "daemon_reliability_start_count";
const RELIABILITY_KEY_CLEAN_EXIT_COUNT: &str = "daemon_reliability_clean_exit_count";
const RELIABILITY_KEY_UNEXPECTED_EXIT_COUNT: &str = "daemon_reliability_unexpected_exit_count";
const RELIABILITY_KEY_LAST_START_TS: &str = "daemon_reliability_last_start_ts";
const RELIABILITY_KEY_LAST_EXIT_TS: &str = "daemon_reliability_last_exit_ts";
const RELIABILITY_KEY_LAST_ERROR_TS: &str = "daemon_reliability_last_error_ts";
const RELIABILITY_KEY_LAST_ERROR_STAGE: &str = "daemon_reliability_last_error_stage";
const RELIABILITY_KEY_LAST_ERROR_MESSAGE: &str = "daemon_reliability_last_error_message";
const RELIABILITY_KEY_POLL_ERROR_COUNT: &str = "daemon_reliability_poll_error_count";
const RELIABILITY_KEY_IPC_ERROR_COUNT: &str = "daemon_reliability_ipc_error_count";

#[derive(Clone)]
pub struct Db {
    path: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
struct ActiveCapDefinition {
    id: i64,
    scope: String,
    interface_guid: Option<String>,
    monthly_cap_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct RetentionCleanupStatus {
    pub last_run_ts: i64,
    pub cutoff_ts: i64,
    pub deleted_usage_records: u64,
    pub deleted_afk_windows: u64,
    pub last_result: String,
}

#[derive(Debug, Clone)]
pub struct ReliabilityStatus {
    pub daemon_start_count: u64,
    pub daemon_clean_exit_count: u64,
    pub daemon_unexpected_exit_count: u64,
    pub daemon_last_start_ts: i64,
    pub daemon_last_exit_ts: i64,
    pub daemon_last_error_ts: i64,
    pub daemon_last_error_stage: String,
    pub daemon_last_error_message: String,
    pub poll_error_count: u64,
    pub ipc_error_count: u64,
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
        let onboarding_completed =
            read_setting_bool(&conn, "onboarding_completed")?.unwrap_or(false);
        let export_default_granularity = normalize_granularity(
            read_setting_string(&conn, "export_default_granularity")?
                .as_deref()
                .unwrap_or("day"),
        )
        .to_string();
        let export_default_include_summary =
            read_setting_bool(&conn, "export_default_include_summary")?.unwrap_or(true);
        let export_default_include_apps =
            read_setting_bool(&conn, "export_default_include_apps")?.unwrap_or(true);
        let export_default_include_interfaces =
            read_setting_bool(&conn, "export_default_include_interfaces")?.unwrap_or(true);

        Ok(SettingsResponse {
            poll_interval_seconds,
            retention_days,
            afk_idle_threshold_seconds,
            onboarding_completed,
            export_default_granularity,
            export_default_include_summary,
            export_default_include_apps,
            export_default_include_interfaces,
        })
    }

    pub fn get_afk_idle_threshold_seconds(&self, default: u32) -> Result<u32> {
        let conn = self.open_connection()?;
        let threshold = read_setting_u32(&conn, "afk_idle_threshold_seconds")?
            .unwrap_or(default)
            .clamp(30, 3600);
        Ok(threshold)
    }

    pub fn get_retention_cleanup_status(&self) -> Result<RetentionCleanupStatus> {
        let conn = self.open_connection()?;
        Ok(RetentionCleanupStatus {
            last_run_ts: read_setting_i64(&conn, RETENTION_KEY_LAST_RUN_TS)?.unwrap_or(0),
            cutoff_ts: read_setting_i64(&conn, RETENTION_KEY_CUTOFF_TS)?.unwrap_or(0),
            deleted_usage_records: read_setting_u64(&conn, RETENTION_KEY_DELETED_USAGE_RECORDS)?
                .unwrap_or(0),
            deleted_afk_windows: read_setting_u64(&conn, RETENTION_KEY_DELETED_AFK_WINDOWS)?
                .unwrap_or(0),
            last_result: read_setting_string(&conn, RETENTION_KEY_LAST_RESULT)?
                .unwrap_or_else(|| "never".to_string()),
        })
    }

    pub fn get_reliability_status(&self) -> Result<ReliabilityStatus> {
        let conn = self.open_connection()?;
        Ok(ReliabilityStatus {
            daemon_start_count: read_setting_u64(&conn, RELIABILITY_KEY_START_COUNT)?.unwrap_or(0),
            daemon_clean_exit_count: read_setting_u64(&conn, RELIABILITY_KEY_CLEAN_EXIT_COUNT)?
                .unwrap_or(0),
            daemon_unexpected_exit_count: read_setting_u64(
                &conn,
                RELIABILITY_KEY_UNEXPECTED_EXIT_COUNT,
            )?
            .unwrap_or(0),
            daemon_last_start_ts: read_setting_i64(&conn, RELIABILITY_KEY_LAST_START_TS)?
                .unwrap_or(0),
            daemon_last_exit_ts: read_setting_i64(&conn, RELIABILITY_KEY_LAST_EXIT_TS)?
                .unwrap_or(0),
            daemon_last_error_ts: read_setting_i64(&conn, RELIABILITY_KEY_LAST_ERROR_TS)?
                .unwrap_or(0),
            daemon_last_error_stage: read_setting_string(&conn, RELIABILITY_KEY_LAST_ERROR_STAGE)?
                .unwrap_or_default(),
            daemon_last_error_message: read_setting_string(
                &conn,
                RELIABILITY_KEY_LAST_ERROR_MESSAGE,
            )?
            .unwrap_or_default(),
            poll_error_count: read_setting_u64(&conn, RELIABILITY_KEY_POLL_ERROR_COUNT)?
                .unwrap_or(0),
            ipc_error_count: read_setting_u64(&conn, RELIABILITY_KEY_IPC_ERROR_COUNT)?.unwrap_or(0),
        })
    }

    pub fn mark_daemon_start(&self, ts: i64) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        let session_open = read_setting_bool(&tx, RELIABILITY_KEY_SESSION_OPEN)?.unwrap_or(false);
        if session_open {
            increment_setting_u64(&tx, RELIABILITY_KEY_UNEXPECTED_EXIT_COUNT, ts)?;
        }

        increment_setting_u64(&tx, RELIABILITY_KEY_START_COUNT, ts)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_START_TS, &ts.to_string(), ts)?;
        upsert_setting(&tx, RELIABILITY_KEY_SESSION_OPEN, "1", ts)?;
        tx.commit()?;
        Ok(())
    }

    pub fn mark_daemon_clean_exit(&self, ts: i64) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        increment_setting_u64(&tx, RELIABILITY_KEY_CLEAN_EXIT_COUNT, ts)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_EXIT_TS, &ts.to_string(), ts)?;
        upsert_setting(&tx, RELIABILITY_KEY_SESSION_OPEN, "0", ts)?;
        tx.commit()?;
        Ok(())
    }

    pub fn increment_poll_error_count(&self, ts: i64, message: &str) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        increment_setting_u64(&tx, RELIABILITY_KEY_POLL_ERROR_COUNT, ts)?;
        write_reliability_error(&tx, ts, "poll", message)?;
        tx.commit()?;
        Ok(())
    }

    pub fn increment_ipc_error_count(&self, ts: i64, message: &str) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        increment_setting_u64(&tx, RELIABILITY_KEY_IPC_ERROR_COUNT, ts)?;
        write_reliability_error(&tx, ts, "ipc", message)?;
        tx.commit()?;
        Ok(())
    }

    pub fn record_daemon_error(&self, ts: i64, stage: &str, message: &str) -> Result<()> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        write_reliability_error(&tx, ts, stage, message)?;
        tx.commit()?;
        Ok(())
    }

    pub fn run_retention_cleanup_if_due(&self, now_ts: i64) -> Result<()> {
        if now_ts <= 0 {
            return Ok(());
        }

        let mut conn = self.open_connection()?;
        let retention_days = read_setting_u32(&conn, "retention_days")?
            .unwrap_or(0)
            .min(3650);
        let current_day_start = utc_day_start_ts(now_ts);
        let last_run_day_start =
            read_setting_i64(&conn, RETENTION_KEY_LAST_RUN_DAY_START_TS)?.unwrap_or(0);

        if last_run_day_start >= current_day_start {
            return Ok(());
        }

        if retention_days > 0 {
            let cutoff_ts = current_day_start
                .saturating_sub(i64::from(retention_days).saturating_mul(24 * 3600));
            let tx = conn.transaction()?;
            let deleted_usage_records = tx.execute(
                "DELETE FROM usage_records WHERE ts < ?1",
                params![cutoff_ts],
            )? as u64;
            let deleted_afk_windows = tx.execute(
                "DELETE FROM afk_windows WHERE end_ts < ?1",
                params![cutoff_ts],
            )? as u64;
            upsert_setting(
                &tx,
                RETENTION_KEY_DELETED_USAGE_RECORDS,
                &deleted_usage_records.to_string(),
                now_ts,
            )?;
            upsert_setting(
                &tx,
                RETENTION_KEY_DELETED_AFK_WINDOWS,
                &deleted_afk_windows.to_string(),
                now_ts,
            )?;
            upsert_setting(&tx, RETENTION_KEY_LAST_RESULT, "ok", now_ts)?;
            upsert_setting(&tx, RETENTION_KEY_CUTOFF_TS, &cutoff_ts.to_string(), now_ts)?;
            upsert_setting(&tx, RETENTION_KEY_LAST_RUN_TS, &now_ts.to_string(), now_ts)?;
            upsert_setting(
                &tx,
                RETENTION_KEY_LAST_RUN_DAY_START_TS,
                &current_day_start.to_string(),
                now_ts,
            )?;
            tx.commit()?;
            return Ok(());
        }

        let tx = conn.transaction()?;
        upsert_setting(&tx, RETENTION_KEY_DELETED_USAGE_RECORDS, "0", now_ts)?;
        upsert_setting(&tx, RETENTION_KEY_DELETED_AFK_WINDOWS, "0", now_ts)?;
        upsert_setting(&tx, RETENTION_KEY_LAST_RESULT, "skipped_unlimited", now_ts)?;
        upsert_setting(&tx, RETENTION_KEY_CUTOFF_TS, "0", now_ts)?;
        upsert_setting(&tx, RETENTION_KEY_LAST_RUN_TS, &now_ts.to_string(), now_ts)?;
        upsert_setting(
            &tx,
            RETENTION_KEY_LAST_RUN_DAY_START_TS,
            &current_day_start.to_string(),
            now_ts,
        )?;
        tx.commit()?;

        Ok(())
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

        if let Some(onboarding_completed) = settings.onboarding_completed {
            let value = if onboarding_completed { "1" } else { "0" };
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('onboarding_completed', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        if let Some(granularity) = settings.export_default_granularity.as_deref() {
            let value = normalize_granularity(granularity);
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('export_default_granularity', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        if let Some(include_summary) = settings.export_default_include_summary {
            let value = if include_summary { "1" } else { "0" };
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('export_default_include_summary', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        if let Some(include_apps) = settings.export_default_include_apps {
            let value = if include_apps { "1" } else { "0" };
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('export_default_include_apps', ?1, ?2)
                ON CONFLICT(key)
                DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
                ",
                params![value, ts],
            )?;
        }

        if let Some(include_interfaces) = settings.export_default_include_interfaces {
            let value = if include_interfaces { "1" } else { "0" };
            tx.execute(
                "
                INSERT INTO settings(key, value, updated_at)
                VALUES('export_default_include_interfaces', ?1, ?2)
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
        let order_by = resolve_app_breakdown_order_by(req.sort_by.as_deref());
        let sql = format!(
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
            ORDER BY {order_by}
            LIMIT :limit
            "
        );
        let mut stmt = conn.prepare(&sql)?;

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

    pub fn list_cap_definitions(&self) -> Result<ListCapDefinitionsResponse> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "
            SELECT id, scope, interface_guid, monthly_cap_bytes, is_active, created_at, updated_at
            FROM monthly_cap_definitions
            ORDER BY scope ASC, COALESCE(interface_guid, ''), id ASC
            ",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(CapDefinition {
                id: row.get::<_, i64>(0)?,
                scope: row.get::<_, String>(1)?,
                interface_guid: row.get::<_, Option<String>>(2)?,
                monthly_cap_bytes: row.get::<_, i64>(3)?.max(0) as u64,
                is_active: row.get::<_, i64>(4)? != 0,
                created_at: row.get::<_, i64>(5)?,
                updated_at: row.get::<_, i64>(6)?,
            })
        })?;

        let caps = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ListCapDefinitionsResponse { caps })
    }

    pub fn upsert_cap_definition(
        &self,
        ts: i64,
        payload: &UpsertCapDefinitionRequest,
    ) -> Result<UpsertCapDefinitionResponse> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        let normalized_scope = match payload.scope.trim().to_ascii_lowercase().as_str() {
            CAP_SCOPE_GLOBAL => CAP_SCOPE_GLOBAL,
            CAP_SCOPE_INTERFACE => CAP_SCOPE_INTERFACE,
            _ => anyhow::bail!("invalid scope"),
        };
        let normalized_interface_guid =
            normalize_cap_interface_guid(normalized_scope, payload.interface_guid.as_deref())?;
        let monthly_cap_bytes = payload.monthly_cap_bytes.clamp(1, i64::MAX as u64) as i64;
        let is_active = if payload.is_active { 1_i64 } else { 0_i64 };

        if let Some(id) = payload.id {
            tx.execute(
                "
                UPDATE monthly_cap_definitions
                SET scope = ?2,
                    interface_guid = ?3,
                    monthly_cap_bytes = ?4,
                    is_active = ?5,
                    updated_at = ?6
                WHERE id = ?1
                ",
                params![
                    id,
                    normalized_scope,
                    normalized_interface_guid,
                    monthly_cap_bytes,
                    is_active,
                    ts,
                ],
            )?;
        }

        tx.execute(
            "
            INSERT INTO monthly_cap_definitions(
                scope,
                interface_guid,
                monthly_cap_bytes,
                is_active,
                created_at,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ON CONFLICT(scope, interface_guid)
            DO UPDATE SET
                monthly_cap_bytes = excluded.monthly_cap_bytes,
                is_active = excluded.is_active,
                updated_at = excluded.updated_at
            ",
            params![
                normalized_scope,
                normalized_interface_guid,
                monthly_cap_bytes,
                is_active,
                ts,
            ],
        )?;

        let cap = tx.query_row(
            "
            SELECT id, scope, interface_guid, monthly_cap_bytes, is_active, created_at, updated_at
            FROM monthly_cap_definitions
            WHERE scope = ?1
              AND ((interface_guid IS NULL AND ?2 IS NULL) OR interface_guid = ?2)
            LIMIT 1
            ",
            params![normalized_scope, normalized_interface_guid],
            |row| {
                Ok(CapDefinition {
                    id: row.get::<_, i64>(0)?,
                    scope: row.get::<_, String>(1)?,
                    interface_guid: row.get::<_, Option<String>>(2)?,
                    monthly_cap_bytes: row.get::<_, i64>(3)?.max(0) as u64,
                    is_active: row.get::<_, i64>(4)? != 0,
                    created_at: row.get::<_, i64>(5)?,
                    updated_at: row.get::<_, i64>(6)?,
                })
            },
        )?;

        tx.commit()?;
        Ok(UpsertCapDefinitionResponse { cap })
    }

    pub fn delete_cap_definition(
        &self,
        payload: &DeleteCapDefinitionRequest,
    ) -> Result<DeleteCapDefinitionResponse> {
        let conn = self.open_connection()?;
        let deleted = conn.execute(
            "DELETE FROM monthly_cap_definitions WHERE id = ?1",
            params![payload.id],
        )?;
        Ok(DeleteCapDefinitionResponse {
            deleted: deleted > 0,
        })
    }

    pub fn list_cap_alert_events(
        &self,
        req: &ListCapAlertEventsRequest,
    ) -> Result<ListCapAlertEventsResponse> {
        let conn = self.open_connection()?;
        let limit = req.limit.unwrap_or(200).clamp(1, 1000);
        let scope = normalize_cap_alert_scope_filter(req.scope.as_deref());
        let interface_guid = req
            .interface_guid
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let window_kind = normalize_cap_alert_window_filter(req.window_kind.as_deref());
        let threshold_kind = normalize_cap_alert_threshold_filter(req.threshold_kind.as_deref());
        let delivery_state = normalize_cap_alert_delivery_filter(req.delivery_state.as_deref());

        let mut stmt = conn.prepare(
            "
            SELECT
                id,
                cap_definition_id,
                scope,
                interface_guid,
                window_kind,
                window_start_ts,
                window_end_ts,
                threshold_kind,
                threshold_value,
                usage_bytes,
                cap_bytes,
                fired_at,
                delivery_state,
                delivered_at
            FROM cap_alert_events
            WHERE (:start_ts IS NULL OR fired_at >= :start_ts)
              AND (:end_ts IS NULL OR fired_at < :end_ts)
              AND (:scope IS NULL OR scope = :scope)
              AND (:interface_guid IS NULL OR interface_guid = :interface_guid)
              AND (:window_kind IS NULL OR window_kind = :window_kind)
              AND (:threshold_kind IS NULL OR threshold_kind = :threshold_kind)
              AND (:delivery_state IS NULL OR delivery_state = :delivery_state)
            ORDER BY fired_at DESC, id DESC
            LIMIT :limit
            ",
        )?;

        let rows = stmt.query_map(
            named_params! {
                ":start_ts": req.start_ts,
                ":end_ts": req.end_ts,
                ":scope": scope.as_deref(),
                ":interface_guid": interface_guid,
                ":window_kind": window_kind.as_deref(),
                ":threshold_kind": threshold_kind.as_deref(),
                ":delivery_state": delivery_state.as_deref(),
                ":limit": i64::from(limit),
            },
            |row| {
                Ok(CapAlertEvent {
                    id: row.get::<_, i64>(0)?,
                    cap_definition_id: row.get::<_, i64>(1)?,
                    scope: row.get::<_, String>(2)?,
                    interface_guid: row.get::<_, Option<String>>(3)?,
                    window_kind: row.get::<_, String>(4)?,
                    window_start_ts: row.get::<_, i64>(5)?,
                    window_end_ts: row.get::<_, i64>(6)?,
                    threshold_kind: row.get::<_, String>(7)?,
                    threshold_value: row.get::<_, i64>(8)?.max(0) as u64,
                    usage_bytes: row.get::<_, i64>(9)?.max(0) as u64,
                    cap_bytes: row.get::<_, i64>(10)?.max(0) as u64,
                    fired_at: row.get::<_, i64>(11)?,
                    delivery_state: row.get::<_, String>(12)?,
                    delivered_at: row.get::<_, Option<i64>>(13)?,
                })
            },
        )?;

        let events = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ListCapAlertEventsResponse { events })
    }

    pub fn mark_cap_alert_events_delivered(
        &self,
        event_ids: &[i64],
        delivered_at: i64,
    ) -> Result<MarkCapAlertEventsDeliveredResponse> {
        if event_ids.is_empty() {
            return Ok(MarkCapAlertEventsDeliveredResponse { updated: 0 });
        }

        let mut ids = event_ids
            .iter()
            .copied()
            .filter(|id| *id > 0)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(MarkCapAlertEventsDeliveredResponse { updated: 0 });
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare(
            "
            UPDATE cap_alert_events
            SET delivery_state = 'delivered',
                delivered_at = ?2
            WHERE id = ?1
              AND delivery_state = 'new'
            ",
        )?;

        let mut updated = 0u32;
        for id in ids {
            let row_count = stmt.execute(params![id, delivered_at])?;
            updated = updated.saturating_add(row_count as u32);
        }
        drop(stmt);

        tx.commit()?;
        Ok(MarkCapAlertEventsDeliveredResponse { updated })
    }

    pub fn evaluate_cap_alerts(&self, now_ts: i64) -> Result<usize> {
        if now_ts <= 0 {
            return Ok(0);
        }

        let conn = self.open_connection()?;
        let caps = list_active_cap_definitions(&conn)?;
        if caps.is_empty() {
            return Ok(0);
        }

        let month_start_ts = utc_month_start_ts(&conn, now_ts)?;
        let day_start_ts = utc_day_start_ts(now_ts);
        let end_ts = now_ts.saturating_add(1);
        let mut inserted = 0usize;

        for cap in caps {
            let monthly_usage = query_cap_usage_bytes(
                &conn,
                month_start_ts,
                end_ts,
                cap.interface_guid.as_deref(),
            )?;

            for threshold_percent in CAP_ALERT_THRESHOLDS_PCT {
                let threshold_bytes =
                    threshold_bytes_for_percent(cap.monthly_cap_bytes, threshold_percent);
                if threshold_bytes == 0 || monthly_usage < threshold_bytes {
                    continue;
                }

                let threshold_kind = match threshold_percent {
                    50 => "pct_50",
                    80 => "pct_80",
                    95 => "pct_95",
                    _ => continue,
                };

                let was_inserted = insert_cap_alert_event(
                    &conn,
                    cap.id,
                    &cap.scope,
                    cap.interface_guid.as_deref(),
                    CAP_ALERT_WINDOW_MONTHLY,
                    month_start_ts,
                    end_ts,
                    threshold_kind,
                    threshold_percent,
                    monthly_usage,
                    cap.monthly_cap_bytes,
                    now_ts,
                )?;
                if was_inserted {
                    inserted = inserted.saturating_add(1);
                }
            }

            let daily_cap_bytes = derive_daily_cap_bytes(cap.monthly_cap_bytes);
            let daily_usage =
                query_cap_usage_bytes(&conn, day_start_ts, end_ts, cap.interface_guid.as_deref())?;
            if daily_usage >= daily_cap_bytes {
                let was_inserted = insert_cap_alert_event(
                    &conn,
                    cap.id,
                    &cap.scope,
                    cap.interface_guid.as_deref(),
                    CAP_ALERT_WINDOW_DAILY,
                    day_start_ts,
                    end_ts,
                    "daily_cap",
                    daily_cap_bytes,
                    daily_usage,
                    daily_cap_bytes,
                    now_ts,
                )?;
                if was_inserted {
                    inserted = inserted.saturating_add(1);
                }
            }
        }

        Ok(inserted)
    }

    pub fn query_afk_audit(&self, req: &GetAfkAuditRequest) -> Result<AfkAuditResponse> {
        let conn = self.open_connection()?;
        let limit = req.limit.unwrap_or(100).clamp(1, 1000);
        let mut stmt = conn.prepare(
            "
            SELECT start_ts, end_ts
            FROM afk_windows
            WHERE (:start_ts IS NULL OR end_ts >= :start_ts)
              AND (:end_ts IS NULL OR start_ts < :end_ts)
            ORDER BY start_ts DESC
            LIMIT :limit
            ",
        )?;

        let windows = stmt.query_map(
            named_params! {
                ":start_ts": req.start_ts,
                ":end_ts": req.end_ts,
                ":limit": i64::from(limit),
            },
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;

        let mut afk_windows = Vec::new();
        for window in windows {
            let (start_ts, end_ts) = window?;
            let duration_seconds = end_ts
                .saturating_sub(start_ts)
                .clamp(0, i64::from(u32::MAX)) as u32;
            let exclusive_end = end_ts.saturating_add(1);

            let (bytes_sent, bytes_recv) = conn.query_row(
                "
                WITH helper_cutover AS (
                    SELECT MIN(ts) AS ts
                    FROM usage_records
                    WHERE source = 'helper'
                )
                SELECT
                    COALESCE(SUM(ur.bytes_sent), 0) AS sent,
                    COALESCE(SUM(ur.bytes_recv), 0) AS recv
                FROM usage_records ur
                CROSS JOIN helper_cutover hc
                WHERE ur.ts >= ?1
                  AND ur.ts < ?2
                  AND (
                      ur.source = 'helper'
                      OR (ur.source = 'import' AND (hc.ts IS NULL OR ur.ts < hc.ts))
                  )
                ",
                params![start_ts, exclusive_end],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as u64,
                        row.get::<_, i64>(1)?.max(0) as u64,
                    ))
                },
            )?;

            let mut top_stmt = conn.prepare(
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
                CROSS JOIN helper_cutover hc
                WHERE ur.ts >= ?1
                  AND ur.ts < ?2
                  AND (
                      ur.source = 'helper'
                      OR (ur.source = 'import' AND (hc.ts IS NULL OR ur.ts < hc.ts))
                  )
                GROUP BY a.id, a.process_name, display_name
                ORDER BY (sent + recv) DESC
                LIMIT 5
                ",
            )?;

            let top_rows = top_stmt.query_map(params![start_ts, exclusive_end], |row| {
                Ok(AppUsageRow {
                    process_name: row.get(0)?,
                    display_name: row.get(1)?,
                    bytes_sent: row.get::<_, i64>(2)?.max(0) as u64,
                    bytes_recv: row.get::<_, i64>(3)?.max(0) as u64,
                    last_seen_ts: row.get::<_, i64>(4)?,
                })
            })?;

            afk_windows.push(AfkWindowUsage {
                start_ts,
                end_ts,
                duration_seconds,
                bytes_sent,
                bytes_recv,
                top_apps: top_rows.collect::<std::result::Result<Vec<_>, _>>()?,
            });
        }

        Ok(AfkAuditResponse { afk_windows })
    }

    pub fn upsert_afk_window(&self, start_ts: i64, end_ts: i64, source: &str) -> Result<()> {
        if start_ts <= 0 || end_ts <= 0 || end_ts < start_ts {
            return Ok(());
        }

        let normalized_source = if source.trim().is_empty() {
            "wts"
        } else {
            source.trim()
        };

        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;

        let merge_floor = start_ts.saturating_sub(1);
        let merge_ceiling = end_ts.saturating_add(1);
        let mut stmt = tx.prepare(
            "
            SELECT id, start_ts, end_ts
            FROM afk_windows
            WHERE start_ts <= ?1
              AND end_ts >= ?2
            ORDER BY start_ts ASC
            ",
        )?;

        let mut rows = stmt.query(params![merge_ceiling, merge_floor])?;
        let mut overlapping = Vec::new();
        while let Some(row) = rows.next()? {
            overlapping.push((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ));
        }
        drop(rows);
        drop(stmt);

        if overlapping.is_empty() {
            tx.execute(
                "
                INSERT INTO afk_windows(start_ts, end_ts, source)
                VALUES(?1, ?2, ?3)
                ",
                params![start_ts, end_ts, normalized_source],
            )?;
            tx.commit()?;
            return Ok(());
        }

        let mut merged_start = start_ts;
        let mut merged_end = end_ts;
        for (_, existing_start, existing_end) in &overlapping {
            merged_start = merged_start.min(*existing_start);
            merged_end = merged_end.max(*existing_end);
        }

        let keep_id = overlapping[0].0;
        tx.execute(
            "
            UPDATE afk_windows
            SET start_ts = ?1,
                end_ts = ?2,
                source = ?3
            WHERE id = ?4
            ",
            params![merged_start, merged_end, normalized_source, keep_id],
        )?;

        for (id, _, _) in overlapping.into_iter().skip(1) {
            tx.execute("DELETE FROM afk_windows WHERE id = ?1", params![id])?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn db_size_bytes(&self) -> u64 {
        sqlite_storage_size_bytes(self.path.as_ref())
    }

    pub fn compact_database(&self) -> Result<CompactDatabaseResponse> {
        let before_bytes = self.db_size_bytes();
        let started = Instant::now();

        let conn = self.open_connection()?;
        conn.execute_batch(
            "
            PRAGMA wal_checkpoint(TRUNCATE);
            VACUUM;
            PRAGMA optimize;
            ",
        )?;

        let after_bytes = self.db_size_bytes();
        let reclaimed_bytes = before_bytes.saturating_sub(after_bytes);
        let duration_ms = started.elapsed().as_millis().clamp(0, u128::from(u64::MAX)) as u64;

        Ok(CompactDatabaseResponse {
            before_bytes,
            after_bytes,
            reclaimed_bytes,
            duration_ms,
        })
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
        upsert_setting(&tx, "onboarding_completed", "0", 0)?;
        upsert_setting(&tx, "export_default_granularity", "day", 0)?;
        upsert_setting(&tx, "export_default_include_summary", "1", 0)?;
        upsert_setting(&tx, "export_default_include_apps", "1", 0)?;
        upsert_setting(&tx, "export_default_include_interfaces", "1", 0)?;
        upsert_setting(&tx, "import_status", "idle", 0)?;
        upsert_setting(&tx, "import_progress_pct", "0", 0)?;
        upsert_setting(&tx, RETENTION_KEY_LAST_RUN_TS, "0", 0)?;
        upsert_setting(&tx, RETENTION_KEY_LAST_RUN_DAY_START_TS, "0", 0)?;
        upsert_setting(&tx, RETENTION_KEY_CUTOFF_TS, "0", 0)?;
        upsert_setting(&tx, RETENTION_KEY_DELETED_USAGE_RECORDS, "0", 0)?;
        upsert_setting(&tx, RETENTION_KEY_DELETED_AFK_WINDOWS, "0", 0)?;
        upsert_setting(&tx, RETENTION_KEY_LAST_RESULT, "never", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_SESSION_OPEN, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_START_COUNT, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_CLEAN_EXIT_COUNT, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_UNEXPECTED_EXIT_COUNT, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_START_TS, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_EXIT_TS, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_ERROR_TS, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_ERROR_STAGE, "", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_LAST_ERROR_MESSAGE, "", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_POLL_ERROR_COUNT, "0", 0)?;
        upsert_setting(&tx, RELIABILITY_KEY_IPC_ERROR_COUNT, "0", 0)?;
        tx.commit()?;
        Ok(())
    }
}

fn sqlite_storage_size_bytes(base_path: &Path) -> u64 {
    let mut total = std::fs::metadata(base_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);

    for suffix in ["-wal", "-shm"] {
        let mut sidecar: OsString = base_path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar_path = PathBuf::from(sidecar);
        total = total.saturating_add(
            std::fs::metadata(sidecar_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0),
        );
    }

    total
}

fn normalize_granularity(granularity: &str) -> &str {
    match granularity.trim().to_ascii_lowercase().as_str() {
        "hour" => "hour",
        "week" => "week",
        "month" => "month",
        _ => "day",
    }
}

fn resolve_app_breakdown_order_by(sort_by: Option<&str>) -> &'static str {
    match sort_by
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("name_asc") | Some("display_name_asc") => {
            "display_name COLLATE NOCASE ASC, a.process_name COLLATE NOCASE ASC, (sent + recv) DESC, sent DESC, recv DESC"
        }
        Some("upload_desc") | Some("bytes_sent_desc") => {
            "sent DESC, recv DESC, (sent + recv) DESC, display_name COLLATE NOCASE ASC, a.process_name COLLATE NOCASE ASC"
        }
        Some("download_desc") | Some("bytes_recv_desc") => {
            "recv DESC, sent DESC, (sent + recv) DESC, display_name COLLATE NOCASE ASC, a.process_name COLLATE NOCASE ASC"
        }
        _ => {
            "(sent + recv) DESC, sent DESC, recv DESC, display_name COLLATE NOCASE ASC, a.process_name COLLATE NOCASE ASC"
        }
    }
}

fn normalize_cap_alert_scope_filter(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some(CAP_SCOPE_GLOBAL) => Some(CAP_SCOPE_GLOBAL.to_string()),
        Some(CAP_SCOPE_INTERFACE) => Some(CAP_SCOPE_INTERFACE.to_string()),
        _ => None,
    }
}

fn normalize_cap_alert_window_filter(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some(CAP_ALERT_WINDOW_MONTHLY) => Some(CAP_ALERT_WINDOW_MONTHLY.to_string()),
        Some(CAP_ALERT_WINDOW_DAILY) => Some(CAP_ALERT_WINDOW_DAILY.to_string()),
        _ => None,
    }
}

fn normalize_cap_alert_threshold_filter(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("pct_50") | Some("pct_80") | Some("pct_95") | Some("daily_cap") => {
            raw.map(str::trim).map(str::to_ascii_lowercase)
        }
        _ => None,
    }
}

fn normalize_cap_alert_delivery_filter(raw: Option<&str>) -> Option<String> {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("new") => Some("new".to_string()),
        Some("delivered") => Some("delivered".to_string()),
        _ => None,
    }
}

fn list_active_cap_definitions(conn: &Connection) -> Result<Vec<ActiveCapDefinition>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, scope, interface_guid, monthly_cap_bytes
        FROM monthly_cap_definitions
        WHERE is_active = 1
          AND monthly_cap_bytes > 0
        ORDER BY id ASC
        ",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(ActiveCapDefinition {
            id: row.get::<_, i64>(0)?,
            scope: row.get::<_, String>(1)?,
            interface_guid: row.get::<_, Option<String>>(2)?,
            monthly_cap_bytes: row.get::<_, i64>(3)?.max(0) as u64,
        })
    })?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn query_cap_usage_bytes(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
    interface_guid: Option<&str>,
) -> Result<u64> {
    let total = conn.query_row(
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
            COALESCE(SUM(ur.bytes_sent), 0) + COALESCE(SUM(ur.bytes_recv), 0) AS total_bytes
        FROM usage_records ur
        JOIN interfaces i ON i.id = ur.interface_id
        CROSS JOIN poll_cutover pc
        WHERE ur.ts >= :start_ts
          AND ur.ts < :end_ts
          AND (:interface_guid IS NULL OR i.guid = :interface_guid)
          AND (
              ur.source IN ('interface_poll', 'poll')
              OR (ur.source = 'import' AND (pc.ts IS NULL OR ur.ts < pc.ts))
          )
        ",
        named_params! {
            ":start_ts": start_ts,
            ":end_ts": end_ts,
            ":interface_guid": interface_guid,
        },
        |row| row.get::<_, i64>(0),
    )?;

    Ok(total.max(0) as u64)
}

#[allow(clippy::too_many_arguments)]
fn insert_cap_alert_event(
    conn: &Connection,
    cap_definition_id: i64,
    scope: &str,
    interface_guid: Option<&str>,
    window_kind: &str,
    window_start_ts: i64,
    window_end_ts: i64,
    threshold_kind: &str,
    threshold_value: u64,
    usage_bytes: u64,
    cap_bytes: u64,
    fired_at: i64,
) -> Result<bool> {
    let inserted = conn.execute(
        "
        INSERT INTO cap_alert_events(
            cap_definition_id,
            scope,
            interface_guid,
            window_kind,
            window_start_ts,
            window_end_ts,
            threshold_kind,
            threshold_value,
            usage_bytes,
            cap_bytes,
            fired_at,
            delivery_state
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT(
            cap_definition_id,
            window_kind,
            window_start_ts,
            threshold_kind,
            threshold_value
        )
        DO NOTHING
        ",
        params![
            cap_definition_id,
            scope,
            interface_guid,
            window_kind,
            window_start_ts,
            window_end_ts,
            threshold_kind,
            as_i64_clamped(threshold_value),
            as_i64_clamped(usage_bytes),
            as_i64_clamped(cap_bytes),
            fired_at,
            CAP_ALERT_DELIVERY_NEW,
        ],
    )?;

    Ok(inserted > 0)
}

fn utc_month_start_ts(conn: &Connection, now_ts: i64) -> Result<i64> {
    conn.query_row(
        "
        SELECT COALESCE(
            CAST(strftime('%s', datetime(?1, 'unixepoch', 'start of month')) AS INTEGER),
            ?1
        )
        ",
        params![now_ts],
        |row| row.get::<_, i64>(0),
    )
    .map_err(Into::into)
}

fn utc_day_start_ts(now_ts: i64) -> i64 {
    now_ts - now_ts.rem_euclid(24 * 3600)
}

fn threshold_bytes_for_percent(cap_bytes: u64, percent: u64) -> u64 {
    if cap_bytes == 0 || percent == 0 {
        return 0;
    }

    cap_bytes
        .saturating_mul(percent)
        .saturating_add(99)
        .saturating_div(100)
}

fn derive_daily_cap_bytes(monthly_cap_bytes: u64) -> u64 {
    if monthly_cap_bytes == 0 {
        return 0;
    }

    monthly_cap_bytes.saturating_add(29).saturating_div(30)
}

fn as_i64_clamped(value: u64) -> i64 {
    if value > i64::MAX as u64 {
        i64::MAX
    } else {
        value as i64
    }
}

fn normalize_cap_interface_guid(
    scope: &str,
    interface_guid: Option<&str>,
) -> Result<Option<String>> {
    match scope {
        CAP_SCOPE_GLOBAL => Ok(None),
        CAP_SCOPE_INTERFACE => {
            let trimmed = interface_guid
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("interface scope requires interface_guid"))?;
            Ok(Some(trimmed.to_string()))
        }
        _ => anyhow::bail!("invalid scope"),
    }
}

fn granularity_to_seconds(granularity: &str) -> i64 {
    match normalize_granularity(granularity) {
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

fn increment_setting_u64(conn: &Connection, key: &str, ts: i64) -> Result<u64> {
    let next = read_setting_u64(conn, key)?.unwrap_or(0).saturating_add(1);
    upsert_setting(conn, key, &next.to_string(), ts)?;
    Ok(next)
}

fn write_reliability_error(conn: &Connection, ts: i64, stage: &str, message: &str) -> Result<()> {
    let trimmed_stage = stage.trim();
    let normalized_stage = if trimmed_stage.is_empty() {
        "unknown"
    } else {
        trimmed_stage
    };
    let normalized_message = message.trim();
    let truncated_message = if normalized_message.chars().count() > 240 {
        normalized_message.chars().take(240).collect::<String>()
    } else {
        normalized_message.to_string()
    };

    upsert_setting(conn, RELIABILITY_KEY_LAST_ERROR_TS, &ts.to_string(), ts)?;
    upsert_setting(conn, RELIABILITY_KEY_LAST_ERROR_STAGE, normalized_stage, ts)?;
    upsert_setting(
        conn,
        RELIABILITY_KEY_LAST_ERROR_MESSAGE,
        &truncated_message,
        ts,
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

fn read_setting_i64(conn: &Connection, key: &str) -> Result<Option<i64>> {
    let raw = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(raw.and_then(|value| value.parse::<i64>().ok()))
}

fn read_setting_u64(conn: &Connection, key: &str) -> Result<Option<u64>> {
    let raw = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(raw.and_then(|value| value.parse::<u64>().ok()))
}

fn read_setting_bool(conn: &Connection, key: &str) -> Result<Option<bool>> {
    let raw = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(
        raw.and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }),
    )
}

fn read_setting_string(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1 LIMIT 1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
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

CREATE TABLE IF NOT EXISTS monthly_cap_definitions (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    scope             TEXT NOT NULL,
    interface_guid    TEXT,
    monthly_cap_bytes INTEGER NOT NULL,
    is_active         INTEGER NOT NULL DEFAULT 1,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    CHECK(scope IN ('global', 'interface')),
    CHECK(monthly_cap_bytes > 0),
    CHECK((scope = 'global' AND interface_guid IS NULL) OR (scope = 'interface' AND interface_guid IS NOT NULL))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_monthly_caps_scope_interface
    ON monthly_cap_definitions(scope, interface_guid);
CREATE INDEX IF NOT EXISTS idx_monthly_caps_active
    ON monthly_cap_definitions(is_active, scope);

CREATE TABLE IF NOT EXISTS cap_alert_events (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    cap_definition_id INTEGER NOT NULL REFERENCES monthly_cap_definitions(id) ON DELETE CASCADE,
    scope             TEXT NOT NULL,
    interface_guid    TEXT,
    window_kind       TEXT NOT NULL,
    window_start_ts   INTEGER NOT NULL,
    window_end_ts     INTEGER NOT NULL,
    threshold_kind    TEXT NOT NULL,
    threshold_value   INTEGER NOT NULL,
    usage_bytes       INTEGER NOT NULL,
    cap_bytes         INTEGER NOT NULL,
    fired_at          INTEGER NOT NULL,
    delivery_state    TEXT NOT NULL DEFAULT 'new',
    delivered_at      INTEGER,
    CHECK(window_kind IN ('monthly', 'daily')),
    CHECK(threshold_kind IN ('pct_50', 'pct_80', 'pct_95', 'daily_cap'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cap_alert_events_unique
    ON cap_alert_events(cap_definition_id, window_kind, window_start_ts, threshold_kind, threshold_value);
CREATE INDEX IF NOT EXISTS idx_cap_alert_events_fired_at
    ON cap_alert_events(fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_cap_alert_events_delivery
    ON cap_alert_events(delivery_state, fired_at DESC);

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

    fn new_test_db_path() -> PathBuf {
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
        path
    }

    fn new_test_db() -> Db {
        let path = new_test_db_path();
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

    fn table_exists(conn: &Connection, table_name: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            params![table_name],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists == 1)
        .unwrap_or(false)
    }

    fn index_exists(conn: &Connection, index_name: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
            params![index_name],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists == 1)
        .unwrap_or(false)
    }

    fn column_exists(conn: &Connection, table_name: &str, column_name: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .expect("prepare table_info pragma");
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query table_info")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect table columns");
        columns
            .iter()
            .any(|column| column.eq_ignore_ascii_case(column_name))
    }

    #[test]
    fn initialize_is_idempotent_on_existing_database() {
        let path = new_test_db_path();
        let first = Db::initialize(&path).expect("initialize first db");
        let second = Db::initialize(&path).expect("initialize second db");

        let first_settings = first.get_settings().expect("read first settings");
        let second_settings = second.get_settings().expect("read second settings");

        assert_eq!(
            first_settings.poll_interval_seconds,
            second_settings.poll_interval_seconds
        );
        assert_eq!(
            first_settings.retention_days,
            second_settings.retention_days
        );

        let conn = second.open_connection().expect("open second db");
        assert!(table_exists(&conn, "usage_records"));
        assert!(table_exists(&conn, "cap_alert_events"));
    }

    #[test]
    fn initialize_backfills_missing_tables_and_retention_defaults() {
        let path = new_test_db_path();
        let conn = Connection::open(&path).expect("open raw sqlite db");
        conn.execute_batch(
            "
            CREATE TABLE settings (
                key         TEXT PRIMARY KEY,
                value       TEXT NOT NULL,
                updated_at  INTEGER NOT NULL
            );
            INSERT INTO settings(key, value, updated_at)
            VALUES('poll_interval_seconds', '90', 123);
            ",
        )
        .expect("seed partial schema");

        let db = Db::initialize(&path).expect("initialize db with partial schema");
        let settings = db.get_settings().expect("read settings after init");
        assert_eq!(settings.poll_interval_seconds, 60);

        let status = db
            .get_retention_cleanup_status()
            .expect("read retention cleanup status");
        assert_eq!(status.last_result, "never");

        let conn = db.open_connection().expect("open initialized db");
        assert!(table_exists(&conn, "usage_records"));
        assert!(table_exists(&conn, "afk_windows"));
        assert!(table_exists(&conn, "cap_alert_events"));
    }

    #[test]
    fn schema_contains_required_tables_columns_and_indexes() {
        let db = new_test_db();
        let conn = db.open_connection().expect("open db");

        assert!(table_exists(&conn, "settings"));
        assert!(table_exists(&conn, "usage_records"));
        assert!(table_exists(&conn, "monthly_cap_definitions"));
        assert!(table_exists(&conn, "cap_alert_events"));
        assert!(table_exists(&conn, "afk_windows"));

        assert!(column_exists(
            &conn,
            "monthly_cap_definitions",
            "monthly_cap_bytes"
        ));
        assert!(column_exists(&conn, "cap_alert_events", "delivery_state"));
        assert!(column_exists(&conn, "cap_alert_events", "delivered_at"));
        assert!(column_exists(&conn, "usage_records", "source"));

        assert!(index_exists(&conn, "idx_usage_unique"));
        assert!(index_exists(&conn, "idx_monthly_caps_scope_interface"));
        assert!(index_exists(&conn, "idx_cap_alert_events_unique"));
        assert!(index_exists(&conn, "idx_cap_alert_events_delivery"));
        assert!(index_exists(&conn, "idx_afk_start"));
    }

    #[test]
    fn compact_database_returns_consistent_metrics_and_keeps_db_usable() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;

        for offset in 0..200 {
            insert_usage(
                &db,
                now_ts + offset,
                "compact-sample.exe",
                TEST_ETHERNET_GUID,
                "Ethernet",
                6,
                1000,
                500,
                "interface_poll",
            );
        }

        let result = db.compact_database().expect("compact database");
        assert_eq!(
            result.reclaimed_bytes,
            result.before_bytes.saturating_sub(result.after_bytes)
        );
        assert!(result.duration_ms > 0 || result.before_bytes == result.after_bytes);

        let status = db
            .get_import_status()
            .expect("read import status after compact");
        assert_eq!(status.0, "idle");
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
    fn app_breakdown_respects_sort_modes_and_stable_tiebreakers() {
        let db = new_test_db();
        let ts = 200;

        insert_usage(
            &db,
            ts,
            "zeta.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            50,
            50,
            "helper",
        );
        insert_usage(
            &db,
            ts,
            "alpha.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            50,
            50,
            "helper",
        );
        insert_usage(
            &db,
            ts,
            "beta.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            70,
            30,
            "helper",
        );
        insert_usage(
            &db,
            ts,
            "gamma.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            30,
            70,
            "helper",
        );

        let total = db
            .query_app_breakdown(&AppBreakdownRequest {
                start_ts: 0,
                end_ts: 1000,
                interface_id: None,
                interface_type: None,
                limit: Some(10),
                sort_by: Some("total_bytes_desc".to_string()),
            })
            .expect("query app breakdown total");
        let total_names = total
            .apps
            .iter()
            .map(|row| row.process_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            total_names,
            vec!["beta.exe", "alpha.exe", "zeta.exe", "gamma.exe"]
        );

        let upload = db
            .query_app_breakdown(&AppBreakdownRequest {
                start_ts: 0,
                end_ts: 1000,
                interface_id: None,
                interface_type: None,
                limit: Some(10),
                sort_by: Some("bytes_sent_desc".to_string()),
            })
            .expect("query app breakdown upload");
        let upload_names = upload
            .apps
            .iter()
            .map(|row| row.process_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            upload_names,
            vec!["beta.exe", "alpha.exe", "zeta.exe", "gamma.exe"]
        );

        let download = db
            .query_app_breakdown(&AppBreakdownRequest {
                start_ts: 0,
                end_ts: 1000,
                interface_id: None,
                interface_type: None,
                limit: Some(10),
                sort_by: Some("bytes_recv_desc".to_string()),
            })
            .expect("query app breakdown download");
        let download_names = download
            .apps
            .iter()
            .map(|row| row.process_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            download_names,
            vec!["gamma.exe", "alpha.exe", "zeta.exe", "beta.exe"]
        );

        let by_name = db
            .query_app_breakdown(&AppBreakdownRequest {
                start_ts: 0,
                end_ts: 1000,
                interface_id: None,
                interface_type: None,
                limit: Some(10),
                sort_by: Some("display_name_asc".to_string()),
            })
            .expect("query app breakdown name");
        let by_name_names = by_name
            .apps
            .iter()
            .map(|row| row.process_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            by_name_names,
            vec!["alpha.exe", "beta.exe", "gamma.exe", "zeta.exe"]
        );
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

    #[test]
    fn upsert_and_list_cap_definitions_support_global_and_interface_scopes() {
        let db = new_test_db();
        let ts = 1_700_000_000;

        db.upsert_cap_definition(
            ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "global".to_string(),
                interface_guid: None,
                monthly_cap_bytes: 100 * 1024 * 1024,
                is_active: true,
            },
        )
        .expect("upsert global cap");

        db.upsert_cap_definition(
            ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "interface".to_string(),
                interface_guid: Some(TEST_ETHERNET_GUID.to_string()),
                monthly_cap_bytes: 200 * 1024 * 1024,
                is_active: false,
            },
        )
        .expect("upsert interface cap");

        let caps = db.list_cap_definitions().expect("list caps").caps;
        assert_eq!(caps.len(), 2);
        assert!(
            caps.iter()
                .any(|cap| cap.scope == "global" && cap.interface_guid.is_none())
        );
        assert!(caps.iter().any(|cap| {
            cap.scope == "interface"
                && cap.interface_guid.as_deref() == Some(TEST_ETHERNET_GUID)
                && !cap.is_active
        }));
    }

    #[test]
    fn delete_cap_definition_reports_deleted_row() {
        let db = new_test_db();
        let created = db
            .upsert_cap_definition(
                1_700_000_000,
                &UpsertCapDefinitionRequest {
                    id: None,
                    scope: "global".to_string(),
                    interface_guid: None,
                    monthly_cap_bytes: 300 * 1024 * 1024,
                    is_active: true,
                },
            )
            .expect("create cap")
            .cap;

        let deleted = db
            .delete_cap_definition(&DeleteCapDefinitionRequest { id: created.id })
            .expect("delete cap");
        assert!(deleted.deleted);
        assert!(
            db.list_cap_definitions()
                .expect("list caps")
                .caps
                .is_empty()
        );
    }

    #[test]
    fn evaluate_cap_alerts_inserts_thresholds_once_per_window() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;
        db.upsert_cap_definition(
            now_ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "global".to_string(),
                interface_guid: None,
                monthly_cap_bytes: 1000,
                is_active: true,
            },
        )
        .expect("create global cap");

        insert_usage(
            &db,
            now_ts - 60,
            "cap-threshold-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            400,
            200,
            "interface_poll",
        );

        let inserted_first = db
            .evaluate_cap_alerts(now_ts)
            .expect("evaluate alerts first");
        assert_eq!(inserted_first, 2);

        let inserted_second = db
            .evaluate_cap_alerts(now_ts + 30)
            .expect("evaluate alerts second");
        assert_eq!(inserted_second, 0);

        let conn = db.open_connection().expect("open db");
        let count = conn
            .query_row("SELECT COUNT(*) FROM cap_alert_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count cap alerts");
        assert_eq!(count, 2);

        let mut kinds_stmt = conn
            .prepare("SELECT threshold_kind FROM cap_alert_events ORDER BY threshold_kind")
            .expect("prepare kinds query");
        let kinds = kinds_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query kinds")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect kinds");
        assert_eq!(kinds, vec!["daily_cap".to_string(), "pct_50".to_string()]);
    }

    #[test]
    fn evaluate_cap_alerts_progressively_fires_higher_monthly_thresholds() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;
        db.upsert_cap_definition(
            now_ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "global".to_string(),
                interface_guid: None,
                monthly_cap_bytes: 1000,
                is_active: true,
            },
        )
        .expect("create global cap");

        insert_usage(
            &db,
            now_ts - 120,
            "cap-progress-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            500,
            100,
            "interface_poll",
        );
        assert_eq!(
            db.evaluate_cap_alerts(now_ts)
                .expect("evaluate initial thresholds"),
            2
        );

        insert_usage(
            &db,
            now_ts - 30,
            "cap-progress-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            200,
            100,
            "interface_poll",
        );
        assert_eq!(
            db.evaluate_cap_alerts(now_ts + 30)
                .expect("evaluate 80 threshold"),
            1
        );

        insert_usage(
            &db,
            now_ts + 15,
            "cap-progress-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            60,
            0,
            "interface_poll",
        );
        assert_eq!(
            db.evaluate_cap_alerts(now_ts + 60)
                .expect("evaluate 95 threshold"),
            1
        );

        let conn = db.open_connection().expect("open db");
        let pct_80_count = conn
            .query_row(
                "SELECT COUNT(*) FROM cap_alert_events WHERE threshold_kind = 'pct_80'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count pct_80");
        let pct_95_count = conn
            .query_row(
                "SELECT COUNT(*) FROM cap_alert_events WHERE threshold_kind = 'pct_95'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count pct_95");
        let total_count = conn
            .query_row("SELECT COUNT(*) FROM cap_alert_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count total alerts");

        assert_eq!(pct_80_count, 1);
        assert_eq!(pct_95_count, 1);
        assert_eq!(total_count, 4);
    }

    #[test]
    fn list_cap_alert_events_orders_desc_and_respects_limit() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;
        db.upsert_cap_definition(
            now_ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "global".to_string(),
                interface_guid: None,
                monthly_cap_bytes: 1000,
                is_active: true,
            },
        )
        .expect("create global cap");

        insert_usage(
            &db,
            now_ts - 120,
            "history-order-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            500,
            100,
            "interface_poll",
        );
        db.evaluate_cap_alerts(now_ts)
            .expect("evaluate first thresholds");

        insert_usage(
            &db,
            now_ts - 10,
            "history-order-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            200,
            100,
            "interface_poll",
        );
        db.evaluate_cap_alerts(now_ts + 60)
            .expect("evaluate second thresholds");

        let response = db
            .list_cap_alert_events(&ListCapAlertEventsRequest {
                start_ts: None,
                end_ts: None,
                scope: None,
                interface_guid: None,
                window_kind: None,
                threshold_kind: None,
                delivery_state: None,
                limit: Some(2),
            })
            .expect("list cap alerts");

        assert_eq!(response.events.len(), 2);
        assert!(response.events[0].fired_at >= response.events[1].fired_at);
        assert_eq!(response.events[0].threshold_kind, "pct_80");
    }

    #[test]
    fn list_cap_alert_events_filters_by_scope_and_threshold_kind() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;
        db.upsert_cap_definition(
            now_ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "global".to_string(),
                interface_guid: None,
                monthly_cap_bytes: 1000,
                is_active: true,
            },
        )
        .expect("create global cap");
        db.upsert_cap_definition(
            now_ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "interface".to_string(),
                interface_guid: Some(TEST_ETHERNET_GUID.to_string()),
                monthly_cap_bytes: 1000,
                is_active: true,
            },
        )
        .expect("create interface cap");

        insert_usage(
            &db,
            now_ts - 60,
            "history-filter-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            500,
            100,
            "interface_poll",
        );
        db.evaluate_cap_alerts(now_ts).expect("evaluate thresholds");

        let response = db
            .list_cap_alert_events(&ListCapAlertEventsRequest {
                start_ts: None,
                end_ts: None,
                scope: Some("interface".to_string()),
                interface_guid: Some(TEST_ETHERNET_GUID.to_string()),
                window_kind: None,
                threshold_kind: Some("pct_50".to_string()),
                delivery_state: None,
                limit: Some(10),
            })
            .expect("list filtered alerts");

        assert_eq!(response.events.len(), 1);
        let event = &response.events[0];
        assert_eq!(event.scope, "interface");
        assert_eq!(event.interface_guid.as_deref(), Some(TEST_ETHERNET_GUID));
        assert_eq!(event.threshold_kind, "pct_50");
    }

    #[test]
    fn mark_cap_alert_events_delivered_is_idempotent_and_filterable() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;
        db.upsert_cap_definition(
            now_ts,
            &UpsertCapDefinitionRequest {
                id: None,
                scope: "global".to_string(),
                interface_guid: None,
                monthly_cap_bytes: 1000,
                is_active: true,
            },
        )
        .expect("create cap");

        insert_usage(
            &db,
            now_ts - 120,
            "delivery-state-app.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            500,
            100,
            "interface_poll",
        );
        db.evaluate_cap_alerts(now_ts).expect("evaluate thresholds");

        let pending = db
            .list_cap_alert_events(&ListCapAlertEventsRequest {
                start_ts: None,
                end_ts: None,
                scope: None,
                interface_guid: None,
                window_kind: None,
                threshold_kind: None,
                delivery_state: Some("new".to_string()),
                limit: Some(10),
            })
            .expect("list pending alerts");
        assert_eq!(pending.events.len(), 2);

        let ids = pending
            .events
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>();
        let marked_once = db
            .mark_cap_alert_events_delivered(&ids, now_ts + 10)
            .expect("mark delivered once");
        assert_eq!(marked_once.updated, 2);

        let marked_twice = db
            .mark_cap_alert_events_delivered(&ids, now_ts + 20)
            .expect("mark delivered twice");
        assert_eq!(marked_twice.updated, 0);

        let pending_after = db
            .list_cap_alert_events(&ListCapAlertEventsRequest {
                start_ts: None,
                end_ts: None,
                scope: None,
                interface_guid: None,
                window_kind: None,
                threshold_kind: None,
                delivery_state: Some("new".to_string()),
                limit: Some(10),
            })
            .expect("list pending alerts after mark");
        assert!(pending_after.events.is_empty());

        let delivered = db
            .list_cap_alert_events(&ListCapAlertEventsRequest {
                start_ts: None,
                end_ts: None,
                scope: None,
                interface_guid: None,
                window_kind: None,
                threshold_kind: None,
                delivery_state: Some("delivered".to_string()),
                limit: Some(10),
            })
            .expect("list delivered alerts");
        assert_eq!(delivered.events.len(), 2);
        assert!(
            delivered
                .events
                .iter()
                .all(|event| event.delivered_at.is_some())
        );
    }

    #[test]
    fn retention_cleanup_skips_when_unlimited() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;

        insert_usage(
            &db,
            now_ts - 1_000_000,
            "retention-unlimited.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            100,
            20,
            "interface_poll",
        );
        db.upsert_afk_window(now_ts - 1_000_100, now_ts - 1_000_000, "last_input")
            .expect("insert old afk window");

        db.run_retention_cleanup_if_due(now_ts)
            .expect("run retention cleanup");

        let conn = db.open_connection().expect("open db");
        let usage_count = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count usage rows");
        let afk_count = conn
            .query_row("SELECT COUNT(*) FROM afk_windows", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count afk rows");
        assert_eq!(usage_count, 1);
        assert_eq!(afk_count, 1);

        let status = db
            .get_retention_cleanup_status()
            .expect("get retention cleanup status");
        assert_eq!(status.last_result, "skipped_unlimited");
        assert_eq!(status.deleted_usage_records, 0);
        assert_eq!(status.deleted_afk_windows, 0);
        assert!(status.last_run_ts > 0);
    }

    #[test]
    fn retention_cleanup_deletes_old_rows_and_runs_once_per_day() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;

        db.apply_settings(
            now_ts,
            &SetSettingsRequest {
                poll_interval_seconds: None,
                retention_days: Some(1),
                afk_idle_threshold_seconds: None,
                onboarding_completed: None,
                export_default_granularity: None,
                export_default_include_summary: None,
                export_default_include_apps: None,
                export_default_include_interfaces: None,
            },
        )
        .expect("set retention days");

        let day_start = utc_day_start_ts(now_ts);
        let cutoff_ts = day_start.saturating_sub(24 * 3600);

        insert_usage(
            &db,
            cutoff_ts - 1,
            "retention-old.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            100,
            10,
            "interface_poll",
        );
        insert_usage(
            &db,
            cutoff_ts,
            "retention-boundary.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            100,
            10,
            "interface_poll",
        );
        insert_usage(
            &db,
            cutoff_ts + 1,
            "retention-new.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            100,
            10,
            "interface_poll",
        );

        db.upsert_afk_window(cutoff_ts - 300, cutoff_ts - 200, "last_input")
            .expect("insert old afk window");
        db.upsert_afk_window(cutoff_ts, cutoff_ts + 10, "last_input")
            .expect("insert boundary afk window");
        db.upsert_afk_window(cutoff_ts + 200, cutoff_ts + 260, "last_input")
            .expect("insert new afk window");

        db.run_retention_cleanup_if_due(now_ts)
            .expect("run retention cleanup first");

        let conn = db.open_connection().expect("open db");
        let usage_after_first = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count usage after first cleanup");
        let afk_after_first = conn
            .query_row("SELECT COUNT(*) FROM afk_windows", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count afk after first cleanup");
        assert_eq!(usage_after_first, 2);
        assert_eq!(afk_after_first, 2);

        let first_status = db
            .get_retention_cleanup_status()
            .expect("status after first cleanup");
        assert_eq!(first_status.last_result, "ok");
        assert_eq!(first_status.cutoff_ts, cutoff_ts);
        assert_eq!(first_status.deleted_usage_records, 1);
        assert_eq!(first_status.deleted_afk_windows, 1);

        insert_usage(
            &db,
            cutoff_ts - 2,
            "retention-old-second.exe",
            TEST_ETHERNET_GUID,
            "Ethernet",
            6,
            100,
            10,
            "interface_poll",
        );
        db.upsert_afk_window(cutoff_ts - 900, cutoff_ts - 850, "last_input")
            .expect("insert second old afk window");

        db.run_retention_cleanup_if_due(now_ts + 60)
            .expect("run retention cleanup second");

        let usage_after_second = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count usage after second cleanup");
        let afk_after_second = conn
            .query_row("SELECT COUNT(*) FROM afk_windows", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count afk after second cleanup");
        assert_eq!(usage_after_second, 3);
        assert_eq!(afk_after_second, 3);

        let second_status = db
            .get_retention_cleanup_status()
            .expect("status after second cleanup");
        assert_eq!(second_status.last_run_ts, first_status.last_run_ts);
        assert_eq!(
            second_status.deleted_usage_records,
            first_status.deleted_usage_records
        );
        assert_eq!(
            second_status.deleted_afk_windows,
            first_status.deleted_afk_windows
        );
    }

    #[test]
    fn reliability_status_tracks_starts_clean_exits_and_errors() {
        let db = new_test_db();
        let now_ts = 1_700_000_000;

        db.mark_daemon_start(now_ts).expect("mark first start");
        db.record_daemon_error(now_ts + 5, "init", "startup issue")
            .expect("record daemon error");
        db.increment_poll_error_count(now_ts + 10, "poll issue")
            .expect("increment poll errors");
        db.increment_ipc_error_count(now_ts + 15, "ipc issue")
            .expect("increment ipc errors");

        let mid_status = db
            .get_reliability_status()
            .expect("read mid reliability status");
        assert_eq!(mid_status.daemon_start_count, 1);
        assert_eq!(mid_status.daemon_unexpected_exit_count, 0);
        assert_eq!(mid_status.poll_error_count, 1);
        assert_eq!(mid_status.ipc_error_count, 1);
        assert_eq!(mid_status.daemon_last_error_stage, "ipc");
        assert_eq!(mid_status.daemon_last_error_message, "ipc issue");

        db.mark_daemon_start(now_ts + 30)
            .expect("mark second start without clean exit");
        db.mark_daemon_clean_exit(now_ts + 60)
            .expect("mark clean exit");

        let final_status = db
            .get_reliability_status()
            .expect("read final reliability status");
        assert_eq!(final_status.daemon_start_count, 2);
        assert_eq!(final_status.daemon_clean_exit_count, 1);
        assert_eq!(final_status.daemon_unexpected_exit_count, 1);
        assert_eq!(final_status.daemon_last_start_ts, now_ts + 30);
        assert_eq!(final_status.daemon_last_exit_ts, now_ts + 60);
    }

    #[test]
    fn upsert_afk_window_merges_contiguous_ranges() {
        let db = new_test_db();

        db.upsert_afk_window(100, 130, "last_input")
            .expect("insert first afk window");
        db.upsert_afk_window(131, 170, "last_input")
            .expect("merge contiguous afk window");

        let conn = db.open_connection().expect("open db");
        let count = conn
            .query_row("SELECT COUNT(*) FROM afk_windows", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("count afk windows");
        assert_eq!(count, 1);

        let merged = conn
            .query_row(
                "SELECT start_ts, end_ts FROM afk_windows LIMIT 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("read merged afk window");
        assert_eq!(merged, (100, 170));
    }

    #[test]
    fn afk_audit_includes_duration_totals_and_top_apps() {
        let db = new_test_db();
        let afk_start = 1_000;
        let afk_end = 1_060;

        db.upsert_afk_window(afk_start, afk_end, "last_input")
            .expect("insert afk window");
        insert_usage(
            &db,
            1_010,
            "sm_afk_a.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            100,
            40,
            "import",
        );
        insert_usage(
            &db,
            1_040,
            "sm_afk_b.exe",
            TEST_ATTRIBUTION_GUID,
            "Attributed Usage",
            0,
            200,
            60,
            "helper",
        );

        let response = db
            .query_afk_audit(&GetAfkAuditRequest {
                start_ts: None,
                end_ts: None,
                limit: Some(100),
            })
            .expect("query afk audit");
        assert_eq!(response.afk_windows.len(), 1);

        let window = &response.afk_windows[0];
        assert_eq!(window.start_ts, afk_start);
        assert_eq!(window.end_ts, afk_end);
        assert_eq!(window.duration_seconds, 60);
        assert_eq!(window.bytes_sent, 300);
        assert_eq!(window.bytes_recv, 100);
        assert_eq!(window.top_apps.len(), 2);
    }

    #[test]
    fn afk_audit_respects_time_range_filters() {
        let db = new_test_db();
        db.upsert_afk_window(1_000, 1_030, "last_input")
            .expect("insert first afk window");
        db.upsert_afk_window(2_000, 2_030, "last_input")
            .expect("insert second afk window");

        let filtered = db
            .query_afk_audit(&GetAfkAuditRequest {
                start_ts: Some(1_900),
                end_ts: Some(2_100),
                limit: Some(100),
            })
            .expect("query filtered afk windows");

        assert_eq!(filtered.afk_windows.len(), 1);
        assert_eq!(filtered.afk_windows[0].start_ts, 2_000);
        assert_eq!(filtered.afk_windows[0].end_ts, 2_030);
    }
}
