pub mod macos;

use crate::db::{self, SessionStart};
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CHECKPOINT: Duration = Duration::from_secs(5);

const BLOCKED_BUNDLES: &[&str] = &[
    "com.apple.loginwindow",
    "com.apple.SecurityAgent",
    "com.apple.screensaver.engine",
    "com.apple.WindowManager",
    "com.apple.wallpaper.agent",
];

pub struct TrackerHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl TrackerHandle {
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct Active {
    id: i64,
    bundle_id: String,
    app_name: String,
    title: Option<String>,
    started_at: i64,
    last_seen: i64,
    last_saved: i64,
}

pub fn spawn(state: AppState, interval: Duration, idle_limit: Duration) -> TrackerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let join = thread::Builder::new()
        .name("usage-tracker".into())
        .spawn(move || run(state, interval, idle_limit, stop_flag))
        .expect("spawn tracker thread");
    TrackerHandle {
        stop,
        join: Some(join),
    }
}

fn run(state: AppState, interval: Duration, idle_limit: Duration, stop: Arc<AtomicBool>) {
    let mut active: Option<Active> = None;
    let idle_limit_secs = idle_limit.as_secs_f64();

    {
        let mut status = state.status.lock().unwrap();
        status.running = true;
        status.ax_trusted = macos::ax_trusted();
    }

    while !stop.load(Ordering::Relaxed) {
        let tick_start = Instant::now();
        let now = now_ms();
        let front = macos::frontmost();
        let idle = macos::idle_seconds();
        let ax_trusted = macos::ax_trusted();

        let mut error = None;

        if idle >= idle_limit_secs {
            finalize(&state, &mut active);
        } else if let Some(front) = &front {
            let title = clean_title(front.title.as_deref());
            if BLOCKED_BUNDLES.contains(&front.bundle_id.as_str()) {
                finalize(&state, &mut active);
            } else {
                let same = active.as_ref().is_some_and(|current| {
                    current.bundle_id == front.bundle_id
                        && current.app_name == front.name
                        && current.title == title
                });

                if same {
                    let current = active.as_mut().unwrap();
                    current.last_seen = now;
                    if now - current.last_saved >= CHECKPOINT.as_millis() as i64 {
                        match state.db.lock() {
                            Ok(conn) => {
                                if let Err(err) = db::touch_session(&conn, current.id, now) {
                                    error = Some(err.to_string());
                                }
                            }
                            Err(_) => error = Some("database lock poisoned".into()),
                        }
                        current.last_saved = now;
                    }
                } else {
                    finalize(&state, &mut active);
                    let start = SessionStart {
                        bundle_id: front.bundle_id.clone(),
                        app_name: front.name.clone(),
                        app_path: front.path.clone(),
                        window_title: title.clone(),
                        started_at: now,
                        pid: Some(front.pid),
                    };
                    match state.db.lock() {
                        Ok(conn) => match db::insert_session(&conn, &start) {
                            Ok(id) => {
                                active = Some(Active {
                                    id,
                                    bundle_id: start.bundle_id,
                                    app_name: start.app_name,
                                    title,
                                    started_at: now,
                                    last_seen: now,
                                    last_saved: now,
                                });
                            }
                            Err(err) => error = Some(err.to_string()),
                        },
                        Err(_) => error = Some("database lock poisoned".into()),
                    }
                }
            }
        }

        {
            let mut status = state.status.lock().unwrap();
            status.ax_trusted = ax_trusted;
            status.last_tick_at = Some(now);
            status.idle_seconds = idle;
            status.active = active.is_some();
            status.error = error;
            match &front {
                Some(front) => {
                    status.front_app = Some(front.name.clone());
                    status.front_bundle = Some(front.bundle_id.clone());
                    status.front_title = front.title.clone();
                    status.front_pid = Some(front.pid);
                }
                None => {
                    status.front_app = None;
                    status.front_bundle = None;
                    status.front_title = None;
                    status.front_pid = None;
                }
            }
            status.front_since = active.as_ref().map(|current| current.started_at);
        }

        let elapsed = tick_start.elapsed();
        nap(interval.saturating_sub(elapsed), &stop);
    }

    finalize(&state, &mut active);
    let mut status = state.status.lock().unwrap();
    status.active = false;
    status.running = false;
    status.front_since = None;
}

fn finalize(state: &AppState, active: &mut Option<Active>) {
    if let Some(session) = active.take() {
        let ended_at = session.last_seen.max(session.started_at);
        let duration = ended_at - session.started_at;
        if let Ok(conn) = state.db.lock() {
            if duration < 1000 {
                let _ = db::delete_session(&conn, session.id);
            } else {
                let _ = db::touch_session(&conn, session.id, ended_at);
            }
        }
    }
}

fn clean_title(title: Option<&str>) -> Option<String> {
    title
        .map(|raw| raw.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|title| !title.is_empty())
        .map(|title| title.chars().take(300).collect())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn nap(duration: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(50)));
    }
}
