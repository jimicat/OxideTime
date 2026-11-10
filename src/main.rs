mod config;
mod db;
mod demo;
mod model;
mod state;
mod tracker;
mod web;

use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::load();
    std::fs::create_dir_all(&cfg.data_dir)?;
    std::fs::create_dir_all(cfg.icon_dir())?;

    let conn = db::open(&cfg.db_path())?;
    let demo_seeded = match cfg.demo {
        Some(days) => Some((days, demo::seed(&conn, days)?)),
        None => None,
    };
    let ax_trusted = tracker::macos::ax_trusted();
    let status = Arc::new(Mutex::new(model::TrackerStatus {
        running: cfg.track,
        ax_trusted,
        ..Default::default()
    }));

    let state = state::AppState {
        db: Arc::new(Mutex::new(conn)),
        status: status.clone(),
        icon_dir: cfg.icon_dir(),
        started_at: chrono::Utc::now().timestamp_millis(),
    };

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    let tracker_handle = if cfg.track {
        Some(tracker::spawn(
            state.clone(),
            Duration::from_millis(cfg.interval_ms),
            Duration::from_secs(cfg.idle_secs),
        ))
    } else {
        None
    };

    println!();
    println!("  OxideTime · Mac 使用追踪");
    println!("  看板   http://127.0.0.1:{}", cfg.port);
    println!("  数据   {}", cfg.db_path().display());
    if let Some((days, count)) = demo_seeded {
        println!("  演示   近 {days} 天 · {count} 条虚拟会话");
    }
    println!(
        "  采集   {}",
        if cfg.demo.is_some() {
            "已关闭（演示模式）".to_string()
        } else if cfg.track {
            format!(
                "每 {} 秒采样 · 空闲 {} 秒后停止计时",
                cfg.interval_ms / 1000,
                cfg.idle_secs
            )
        } else {
            "已关闭 (--no-track)".to_string()
        }
    );
    println!(
        "  权限   {}",
        if ax_trusted {
            "辅助功能已授权"
        } else {
            "未授权辅助功能（窗口标题不可用）"
        }
    );
    println!("  退出   Ctrl+C");
    println!();

    axum::serve(listener, web::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    if let Some(handle) = tracker_handle {
        handle.shutdown();
    }
    println!("已停止。");
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("\n正在停止…");
}
