use crate::db::{self, SessionFilter, SessionItem};
use crate::model::TrackerStatus;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Local, NaiveDate, TimeZone};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.0.to_string() }));
        (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn with_db<T, F>(state: &AppState, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> anyhow::Result<T> + Send + 'static,
{
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = db
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))?;
        f(&conn)
    })
    .await?;
    result.map_err(ApiError)
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/assets/style.css", get(style_css))
        .route("/assets/app.js", get(app_js))
        .route("/assets/geist-sans.woff2", get(geist_font))
        .route("/api/dashboard", get(dashboard))
        .route("/api/sessions", get(sessions))
        .route("/api/heatmap", get(heatmap))
        .route("/api/status", get(status))
        .route("/api/export.csv", get(export_csv))
        .route("/api/icons/{key}", get(icon))
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn style_css() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../assets/style.css"),
    )
        .into_response()
}

async fn app_js() -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../assets/app.js"),
    )
        .into_response()
}

async fn geist_font() -> Response {
    (
        [
            (header::CONTENT_TYPE, "font/woff2"),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        include_bytes!("../assets/fonts/geist-sans-latin.woff2").as_slice(),
    )
        .into_response()
}

#[derive(Deserialize)]
struct RangeQuery {
    from: Option<String>,
    to: Option<String>,
}

fn parse_date(value: &str) -> anyhow::Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("无效日期: {value}，应为 YYYY-MM-DD"))
}

fn parse_range(query: &RangeQuery) -> anyhow::Result<(NaiveDate, NaiveDate)> {
    let today = db::today();
    let from = match query.from.as_deref() {
        Some(value) if !value.is_empty() => parse_date(value)?,
        _ => today,
    };
    let to = match query.to.as_deref() {
        Some(value) if !value.is_empty() => parse_date(value)?,
        _ => today,
    };
    Ok((from, to.max(from)))
}

#[derive(Serialize)]
struct DashboardResponse {
    from: String,
    to: String,
    summary: db::Summary,
    apps: Vec<db::AppAgg>,
    hourly: Vec<db::HourTotal>,
}

async fn dashboard(
    State(state): State<AppState>,
    Query(query): Query<RangeQuery>,
) -> Result<Json<DashboardResponse>, ApiError> {
    let (from, to) = parse_range(&query)?;
    let (from_ms, to_ms) = db::range_bounds(from, to);

    let (summary, apps, hourly) = with_db(&state, move |conn| {
        let summary = db::summary(conn, from_ms, to_ms)?;
        let apps = db::top_apps(conn, from_ms, to_ms)?;
        let hourly = db::hourly(conn, from_ms, to_ms)?;
        Ok((summary, apps, hourly))
    })
    .await?;

    Ok(Json(DashboardResponse {
        from: from.format("%Y-%m-%d").to_string(),
        to: to.format("%Y-%m-%d").to_string(),
        summary,
        apps,
        hourly,
    }))
}

#[derive(Deserialize)]
struct SessionsQuery {
    from: Option<String>,
    to: Option<String>,
    q: Option<String>,
    bundle: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Serialize)]
struct SessionsResponse {
    total: i64,
    page: u32,
    page_size: u32,
    items: Vec<SessionItem>,
}

async fn sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<SessionsResponse>, ApiError> {
    let (from, to) = parse_range(&RangeQuery {
        from: query.from,
        to: query.to,
    })?;
    let (from_ms, to_ms) = db::range_bounds(from, to);
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 200);
    let filter = SessionFilter {
        from_ms,
        to_ms,
        q: query.q.unwrap_or_default(),
        bundle: query.bundle.unwrap_or_default(),
    };
    let offset = (page as i64 - 1) * page_size as i64;

    let (total, items) = with_db(&state, move |conn| {
        let total = db::session_count(conn, &filter)?;
        let items = db::sessions(conn, &filter, page_size as i64, offset)?;
        Ok((total, items))
    })
    .await?;

    Ok(Json(SessionsResponse {
        total,
        page,
        page_size,
        items,
    }))
}

#[derive(Deserialize)]
struct HeatQuery {
    days: Option<u32>,
}

#[derive(Serialize)]
struct HeatmapResponse {
    days: Vec<db::HeatDay>,
    max_ms: i64,
}

async fn heatmap(
    State(state): State<AppState>,
    Query(query): Query<HeatQuery>,
) -> Result<Json<HeatmapResponse>, ApiError> {
    let days = query.days.unwrap_or(14).clamp(1, 60);
    let end = db::today();
    let start = end - chrono::Duration::days(days as i64 - 1);

    let rows = with_db(&state, move |conn| db::heatmap(conn, start, end)).await?;
    let max_ms = rows.iter().map(|day| day.total_ms).max().unwrap_or(0);
    Ok(Json(HeatmapResponse { days: rows, max_ms }))
}

#[derive(Serialize)]
struct StatusResponse {
    #[serde(flatten)]
    tracker: TrackerStatus,
    started_at: i64,
    track_enabled: bool,
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let tracker = state.status.lock().map(|s| s.clone()).unwrap_or_default();
    Json(StatusResponse {
        track_enabled: tracker.running,
        tracker,
        started_at: state.started_at,
    })
}

#[derive(Deserialize)]
struct ExportQuery {
    from: Option<String>,
    to: Option<String>,
    q: Option<String>,
    bundle: Option<String>,
}

async fn export_csv(
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let (from, to) = parse_range(&RangeQuery {
        from: query.from,
        to: query.to,
    })?;
    let (from_ms, to_ms) = db::range_bounds(from, to);
    let filter = SessionFilter {
        from_ms,
        to_ms,
        q: query.q.unwrap_or_default(),
        bundle: query.bundle.unwrap_or_default(),
    };

    let rows = with_db(&state, move |conn| db::all_sessions(conn, &filter)).await?;

    let mut csv = String::from("\u{feff}开始时间,结束时间,时长秒,应用,窗口标题,Bundle ID\n");
    for row in &rows {
        csv.push_str(&csv_escape(&timestamp(row.started_at, true)));
        csv.push(',');
        csv.push_str(&csv_escape(&timestamp(row.ended_at, true)));
        csv.push(',');
        csv.push_str(&format!("{:.1}", row.duration_ms as f64 / 1000.0));
        csv.push(',');
        csv.push_str(&csv_escape(&row.app_name));
        csv.push(',');
        csv.push_str(&csv_escape(row.window_title.as_deref().unwrap_or("")));
        csv.push(',');
        csv.push_str(&csv_escape(&row.bundle_id));
        csv.push('\n');
    }

    let filename = format!(
        "usage-{}_to_{}.csv",
        from.format("%Y-%m-%d"),
        to.format("%Y-%m-%d")
    );
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        csv,
    )
        .into_response())
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn timestamp(ms: i64, with_date: bool) -> String {
    match Local.timestamp_millis_opt(ms).earliest() {
        Some(dt) if with_date => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        Some(dt) => dt.format("%H:%M:%S").to_string(),
        None => String::new(),
    }
}

async fn icon(State(state): State<AppState>, Path(key): Path<String>) -> Response {
    let cache_path = state.icon_dir.join(cache_name(&key));
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return png_response(bytes);
    }

    let db = state.db.clone();
    let lookup_key = key.clone();
    let app_path = tokio::task::spawn_blocking(move || -> Option<String> {
        let conn = db.lock().ok()?;
        db::app_path(&conn, &lookup_key).ok().flatten()
    })
    .await
    .ok()
    .flatten();

    let Some(app_path) = app_path else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache_path = state.icon_dir.clone().join(cache_name(&key));
    let png = tokio::task::spawn_blocking(move || {
        let bytes = crate::tracker::macos::icon_png(&app_path)?;
        let _ = std::fs::write(cache_path, &bytes);
        Some(bytes)
    })
    .await
    .ok()
    .flatten();

    match png {
        Some(bytes) => png_response(bytes),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn png_response(bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

fn cache_name(key: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let prefix: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(48)
        .collect();
    format!("{prefix}-{hash:016x}.png")
}
