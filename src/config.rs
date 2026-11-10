use std::env;
use std::path::{Path, PathBuf};

pub struct Config {
    pub data_dir: PathBuf,
    pub port: u16,
    pub interval_ms: u64,
    pub idle_secs: u64,
    pub track: bool,
    pub demo: Option<u32>,
}

impl Config {
    pub fn load() -> Self {
        let mut cfg = Config {
            data_dir: default_data_dir(),
            port: env_u64("TRACKER_PORT").map(|v| v as u16).unwrap_or(4321),
            interval_ms: env_u64("TRACKER_INTERVAL_MS").unwrap_or(2000).max(250),
            idle_secs: env_u64("TRACKER_IDLE_SECS").unwrap_or(180),
            track: true,
            demo: None,
        };

        let args: Vec<String> = env::args().skip(1).collect();
        let custom_data_dir = args.iter().any(|arg| arg == "--data-dir");
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--port" => {
                    if let Some(v) = args.get(i + 1).and_then(|v| v.parse().ok()) {
                        cfg.port = v;
                        i += 1;
                    }
                }
                "--interval" => {
                    if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<u64>().ok()) {
                        cfg.interval_ms = v.max(250);
                        i += 1;
                    }
                }
                "--idle" => {
                    if let Some(v) = args.get(i + 1).and_then(|v| v.parse().ok()) {
                        cfg.idle_secs = v;
                        i += 1;
                    }
                }
                "--data-dir" => {
                    if let Some(v) = args.get(i + 1) {
                        cfg.data_dir = PathBuf::from(v);
                        i += 1;
                    }
                }
                "--no-track" => cfg.track = false,
                "--demo" => {
                    let days = args
                        .get(i + 1)
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(30)
                        .clamp(1, 365);
                    if args
                        .get(i + 1)
                        .is_some_and(|v| v.parse::<u32>().is_ok())
                    {
                        i += 1;
                    }
                    cfg.demo = Some(days);
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    eprintln!("未知参数: {other}\n");
                    print_help();
                    std::process::exit(2);
                }
            }
            i += 1;
        }

        if !custom_data_dir {
            migrate_legacy_data_dir(&cfg.data_dir);
        }
        if cfg.demo.is_some() {
            cfg.data_dir = cfg.data_dir.join("demo");
            cfg.track = false;
        }
        cfg
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("tracker.db")
    }

    pub fn icon_dir(&self) -> PathBuf {
        self.data_dir.join("icons")
    }
}

fn env_u64(key: &str) -> Option<u64> {
    env::var(key).ok().and_then(|v| v.parse().ok())
}

fn default_data_dir() -> PathBuf {
    let home = env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    home.join("Library/Application Support/OxideTime")
}

fn migrate_legacy_data_dir(dir: &Path) {
    if dir.exists() {
        return;
    }
    let Some(legacy) = dir.parent().map(|parent| parent.join("RustTracker")) else {
        return;
    };
    if legacy.exists() {
        let _ = std::fs::rename(legacy, dir);
    }
}

fn print_help() {
    println!(
        "OxideTime · macOS 应用使用追踪\n\n\
用法: oxidetime [选项]\n\n\
选项:\n\
  --port <端口>         Web 看板端口 (默认 4321)\n\
  --interval <秒>       采样间隔秒数 (默认 2)\n\
  --idle <秒>           空闲多久后停止计时 (默认 180)\n\
  --data-dir <目录>     数据目录 (默认 ~/Library/Application Support/OxideTime)\n\
  --no-track            只启动看板，不采集\n\
  --demo [天数]         演示模式：在 <数据目录>/demo 生成近 N 天虚拟数据并关闭采集\n\
  -h, --help            显示帮助\n\n\
环境变量: TRACKER_PORT / TRACKER_INTERVAL_MS / TRACKER_IDLE_SECS"
    );
}
