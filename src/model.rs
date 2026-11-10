use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct TrackerStatus {
    pub running: bool,
    pub ax_trusted: bool,
    pub front_app: Option<String>,
    pub front_bundle: Option<String>,
    pub front_title: Option<String>,
    pub front_since: Option<i64>,
    pub front_pid: Option<i32>,
    pub idle_seconds: f64,
    pub active: bool,
    pub last_tick_at: Option<i64>,
    pub error: Option<String>,
}
