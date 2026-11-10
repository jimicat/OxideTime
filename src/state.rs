use crate::model::TrackerStatus;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub status: Arc<Mutex<TrackerStatus>>,
    pub icon_dir: PathBuf,
    pub started_at: i64,
}
