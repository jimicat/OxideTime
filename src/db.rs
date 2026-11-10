use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone, Timelike};
use rusqlite::{named_params, params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    bundle_id TEXT NOT NULL DEFAULT '',
    app_name TEXT NOT NULL,
    app_path TEXT,
    window_title TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    local_date TEXT NOT NULL,
    start_hour INTEGER NOT NULL,
    pid INTEGER
);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_date ON sessions(local_date);
CREATE INDEX IF NOT EXISTS idx_sessions_bundle ON sessions(bundle_id);
"#;

const SESSION_FILTER: &str = "started_at >= :from AND started_at < :to AND duration_ms >= 1000 \
    AND (:q = '' OR app_name LIKE :like ESCAPE '\\' OR IFNULL(window_title, '') LIKE :like ESCAPE '\\' OR bundle_id LIKE :like ESCAPE '\\') \
    AND (:bundle = '' OR bundle_id = :bundle OR app_name = :bundle)";

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create data dir {}", parent.display()))?;
    }
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=3000;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub struct SessionStart {
    pub bundle_id: String,
    pub app_name: String,
    pub app_path: Option<String>,
    pub window_title: Option<String>,
    pub started_at: i64,
    pub pid: Option<i32>,
}

pub fn insert_session(conn: &Connection, s: &SessionStart) -> Result<i64> {
    let dt = Local
        .timestamp_millis_opt(s.started_at)
        .earliest()
        .unwrap_or_else(Local::now);
    let local_date = dt.format("%Y-%m-%d").to_string();
    let start_hour = dt.hour() as i64;
    conn.execute(
        "INSERT INTO sessions (bundle_id, app_name, app_path, window_title, started_at, ended_at, duration_ms, local_date, start_hour, pid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, 0, ?6, ?7, ?8)",
        params![
            s.bundle_id,
            s.app_name,
            s.app_path,
            s.window_title,
            s.started_at,
            local_date,
            start_hour,
            s.pid
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn touch_session(conn: &Connection, id: i64, ended_at: i64) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET ended_at = ?1, duration_ms = MAX(?1 - started_at, 0) WHERE id = ?2",
        params![ended_at, id],
    )?;
    Ok(())
}

pub fn delete_session(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct AppAgg {
    pub bundle_id: String,
    pub app_name: String,
    pub app_path: Option<String>,
    pub total_ms: i64,
    pub sessions: i64,
    pub last_used: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BusiestHour {
    pub hour: u32,
    pub total_ms: i64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Summary {
    pub total_ms: i64,
    pub app_count: i64,
    pub session_count: i64,
    pub avg_session_ms: i64,
    pub top_app: Option<AppAgg>,
    pub busiest_hour: Option<BusiestHour>,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HourTotal {
    pub date: String,
    pub hour: u32,
    pub total_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HeatDay {
    pub date: String,
    pub total_ms: i64,
    pub hours: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionItem {
    pub id: i64,
    pub bundle_id: String,
    pub app_name: String,
    pub window_title: Option<String>,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
}

#[derive(Clone, Default)]
pub struct SessionFilter {
    pub from_ms: i64,
    pub to_ms: i64,
    pub q: String,
    pub bundle: String,
}

impl SessionFilter {
    fn like(&self) -> String {
        format!("%{}%", escape_like(self.q.trim()))
    }

    fn trimmed_q(&self) -> &str {
        self.q.trim()
    }
}

fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub fn summary(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Summary> {
    let (total_ms, session_count, app_count, first_at, last_at) = conn.query_row(
        "SELECT COALESCE(SUM(duration_ms), 0), COUNT(*),
                COUNT(DISTINCT bundle_id || '|' || app_name), MIN(started_at), MAX(ended_at)
         FROM sessions WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms >= 1000",
        params![from_ms, to_ms],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        },
    )?;

    let apps = top_apps(conn, from_ms, to_ms)?;
    let busiest_hour = conn
        .query_row(
            "SELECT start_hour, SUM(duration_ms) AS total
             FROM sessions WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms >= 1000
             GROUP BY start_hour ORDER BY total DESC LIMIT 1",
            params![from_ms, to_ms],
            |row| {
                Ok(BusiestHour {
                    hour: row.get::<_, i64>(0)? as u32,
                    total_ms: row.get(1)?,
                })
            },
        )
        .optional()?;

    Ok(Summary {
        total_ms,
        app_count,
        session_count,
        avg_session_ms: if session_count > 0 { total_ms / session_count } else { 0 },
        top_app: apps.into_iter().next(),
        busiest_hour,
        first_at,
        last_at,
    })
}

pub fn top_apps(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<AppAgg>> {
    let mut stmt = conn.prepare(
        "SELECT bundle_id, app_name, MAX(app_path), SUM(duration_ms), COUNT(*), MAX(ended_at)
         FROM sessions
         WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms >= 1000
         GROUP BY bundle_id, app_name
         ORDER BY SUM(duration_ms) DESC",
    )?;
    let rows = stmt.query_map(params![from_ms, to_ms], |row| {
        Ok(AppAgg {
            bundle_id: row.get(0)?,
            app_name: row.get(1)?,
            app_path: row.get(2)?,
            total_ms: row.get(3)?,
            sessions: row.get(4)?,
            last_used: row.get(5)?,
        })
    })?;
    let mut apps = Vec::new();
    for row in rows {
        apps.push(row?);
    }
    Ok(apps)
}

pub fn hourly(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<HourTotal>> {
    let mut stmt = conn.prepare(
        "SELECT local_date, start_hour, SUM(duration_ms)
         FROM sessions
         WHERE started_at >= ?1 AND started_at < ?2 AND duration_ms >= 1000
         GROUP BY local_date, start_hour
         ORDER BY local_date, start_hour",
    )?;
    let rows = stmt.query_map(params![from_ms, to_ms], |row| {
        Ok(HourTotal {
            date: row.get(0)?,
            hour: row.get::<_, i64>(1)? as u32,
            total_ms: row.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn heatmap(conn: &Connection, start: NaiveDate, end: NaiveDate) -> Result<Vec<HeatDay>> {
    let start_s = start.format("%Y-%m-%d").to_string();
    let end_s = end.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT local_date, start_hour, SUM(duration_ms)
         FROM sessions
         WHERE local_date >= ?1 AND local_date <= ?2 AND duration_ms >= 1000
         GROUP BY local_date, start_hour",
    )?;
    let rows = stmt.query_map(params![start_s, end_s], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut buckets: HashMap<String, [i64; 24]> = HashMap::new();
    for row in rows {
        let (date, hour, total) = row?;
        let entry = buckets.entry(date).or_insert([0; 24]);
        if (0..24).contains(&hour) {
            entry[hour as usize] += total;
        }
    }

    let mut days = Vec::new();
    let mut cursor = start;
    while cursor <= end {
        let key = cursor.format("%Y-%m-%d").to_string();
        let hours = buckets.remove(&key).unwrap_or([0; 24]);
        days.push(HeatDay {
            date: key,
            total_ms: hours.iter().sum(),
            hours: hours.to_vec(),
        });
        cursor += ChronoDuration::days(1);
    }
    Ok(days)
}

pub fn sessions(conn: &Connection, filter: &SessionFilter, limit: i64, offset: i64) -> Result<Vec<SessionItem>> {
    let sql = format!(
        "SELECT id, bundle_id, app_name, window_title, started_at, ended_at, duration_ms
         FROM sessions WHERE {SESSION_FILTER}
         ORDER BY started_at DESC LIMIT :limit OFFSET :offset"
    );
    let mut stmt = conn.prepare(&sql)?;
    let like = filter.like();
    let rows = stmt.query_map(
        named_params! {
            ":from": filter.from_ms,
            ":to": filter.to_ms,
            ":q": filter.trimmed_q(),
            ":like": like,
            ":bundle": filter.bundle,
            ":limit": limit,
            ":offset": offset,
        },
        session_from_row,
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn session_count(conn: &Connection, filter: &SessionFilter) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM sessions WHERE {SESSION_FILTER}");
    let like = filter.like();
    let count = conn.query_row(
        &sql,
        named_params! {
            ":from": filter.from_ms,
            ":to": filter.to_ms,
            ":q": filter.trimmed_q(),
            ":like": like,
            ":bundle": filter.bundle,
        },
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count)
}

pub fn all_sessions(conn: &Connection, filter: &SessionFilter) -> Result<Vec<SessionItem>> {
    let sql = format!(
        "SELECT id, bundle_id, app_name, window_title, started_at, ended_at, duration_ms
         FROM sessions WHERE {SESSION_FILTER} ORDER BY started_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let like = filter.like();
    let rows = stmt.query_map(
        named_params! {
            ":from": filter.from_ms,
            ":to": filter.to_ms,
            ":q": filter.trimmed_q(),
            ":like": like,
            ":bundle": filter.bundle,
        },
        session_from_row,
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<SessionItem> {
    Ok(SessionItem {
        id: row.get(0)?,
        bundle_id: row.get(1)?,
        app_name: row.get(2)?,
        window_title: row.get(3)?,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        duration_ms: row.get(6)?,
    })
}

pub fn app_path(conn: &Connection, key: &str) -> Result<Option<String>> {
    let path = conn
        .query_row(
            "SELECT app_path FROM sessions
             WHERE (bundle_id = ?1 OR app_name = ?1) AND app_path IS NOT NULL AND app_path <> ''
             ORDER BY started_at DESC LIMIT 1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(path)
}

pub fn day_start_ms(date: NaiveDate) -> i64 {
    let midnight = date.and_hms_opt(0, 0, 0).expect("valid midnight");
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|| midnight.and_utc().timestamp_millis())
}

pub fn range_bounds(from: NaiveDate, to: NaiveDate) -> (i64, i64) {
    let end = to + ChronoDuration::days(1);
    (day_start_ms(from), day_start_ms(end))
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}
