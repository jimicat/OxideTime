use anyhow::Result;
use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Timelike};
use rusqlite::Connection;

use crate::db::{self, SessionStart};

struct AppProfile {
    bundle_id: &'static str,
    name: &'static str,
    path: &'static str,
    weight: u32,
    weekend_weight: u32,
    min_secs: u64,
    max_secs: u64,
    titles: &'static [&'static str],
}

const APPS: &[AppProfile] = &[
    AppProfile {
        bundle_id: "com.google.Chrome",
        name: "Google Chrome",
        path: "/Applications/Google Chrome.app",
        weight: 24,
        weekend_weight: 26,
        min_secs: 20,
        max_secs: 540,
        titles: &[
            "GitHub · oxidetime",
            "Rust 标准库文档",
            "Hacker News",
            "shadcn/ui — 组件文档",
            "Tailwind CSS",
            "Google 搜索",
            "YouTube",
            "Figma · Dashboard Redesign",
            "Notion · 周会记录",
            "MDN Web Docs",
        ],
    },
    AppProfile {
        bundle_id: "com.qoder.app",
        name: "Qoder",
        path: "/Applications/Qoder.app",
        weight: 20,
        weekend_weight: 6,
        min_secs: 45,
        max_secs: 1500,
        titles: &[
            "oxidetime — Qoder",
            "src/web/mod.rs — Qoder",
            "src/db.rs — Qoder",
            "dashboard — Qoder",
            "weekly-report.md — Qoder",
        ],
    },
    AppProfile {
        bundle_id: "com.tencent.xinWeChat",
        name: "WeChat",
        path: "/Applications/WeChat.app",
        weight: 13,
        weekend_weight: 16,
        min_secs: 15,
        max_secs: 300,
        titles: &["微信", "文件传输助手", "产品设计讨论组", "前端技术群"],
    },
    AppProfile {
        bundle_id: "com.mitchellh.ghostty",
        name: "Ghostty",
        path: "/Applications/Ghostty.app",
        weight: 8,
        weekend_weight: 6,
        min_secs: 30,
        max_secs: 900,
        titles: &[
            "zsh — 120×32",
            "cargo run -- --port 4321",
            "cargo watch -x check",
            "git rebase -i main",
            "htop",
        ],
    },
    AppProfile {
        bundle_id: "com.electron.lark",
        name: "Lark",
        path: "/Applications/Lark.app",
        weight: 8,
        weekend_weight: 3,
        min_secs: 30,
        max_secs: 1800,
        titles: &["Lark", "研发周会", "需求评审 · 会议室 A", "Lark Docs · 技术方案"],
    },
    AppProfile {
        bundle_id: "dev.zed.Zed",
        name: "Zed",
        path: "/Applications/Zed.app",
        weight: 6,
        weekend_weight: 2,
        min_secs: 60,
        max_secs: 900,
        titles: &[
            "src/main.rs — Zed",
            "src/tracker/macos.rs — Zed",
            "Cargo.toml — Zed",
        ],
    },
    AppProfile {
        bundle_id: "md.obsidian",
        name: "Obsidian",
        path: "/Applications/Obsidian.app",
        weight: 5,
        weekend_weight: 12,
        min_secs: 60,
        max_secs: 1200,
        titles: &["周报 Draft · Obsidian", "技术笔记 · Obsidian", "Reading list · Obsidian"],
    },
    AppProfile {
        bundle_id: "com.apple.finder",
        name: "Finder",
        path: "/System/Library/CoreServices/Finder.app",
        weight: 4,
        weekend_weight: 6,
        min_secs: 10,
        max_secs: 90,
        titles: &["Downloads", "项目资料", "Applications", "Documents"],
    },
    AppProfile {
        bundle_id: "com.tdesktop.Telegram",
        name: "Telegram",
        path: "/Applications/Telegram.app",
        weight: 3,
        weekend_weight: 10,
        min_secs: 15,
        max_secs: 240,
        titles: &["Telegram", "Saved Messages", "Rust 中文社区"],
    },
    AppProfile {
        bundle_id: "com.apple.Safari",
        name: "Safari",
        path: "/Applications/Safari.app",
        weight: 3,
        weekend_weight: 8,
        min_secs: 45,
        max_secs: 600,
        titles: &["Apple", "Mac 使用技巧", "Rust Blog"],
    },
    AppProfile {
        bundle_id: "id.dbeaver.DBeaver",
        name: "DBeaver",
        path: "/Applications/DBeaver.app",
        weight: 2,
        weekend_weight: 2,
        min_secs: 60,
        max_secs: 720,
        titles: &["tracker.db — DBeaver", "SQL 编辑器 — DBeaver"],
    },
    AppProfile {
        bundle_id: "com.apple.systempreferences",
        name: "System Settings",
        path: "/System/Applications/System Settings.app",
        weight: 2,
        weekend_weight: 1,
        min_secs: 10,
        max_secs: 120,
        titles: &["系统设置", "隐私与安全性", "外观"],
    },
    AppProfile {
        bundle_id: "com.apple.Terminal",
        name: "Terminal",
        path: "/System/Applications/Utilities/Terminal.app",
        weight: 2,
        weekend_weight: 2,
        min_secs: 30,
        max_secs: 480,
        titles: &["zsh — 80×24", "cargo test", "top"],
    },
];

pub fn seed(conn: &Connection, days: u32) -> Result<usize> {
    let days = days.clamp(1, 365) as i64;
    conn.execute("DELETE FROM sessions", [])?;

    let tx = conn.unchecked_transaction()?;
    let today = Local::now().date_naive();
    let now_secs = Local::now().num_seconds_from_midnight() as i64;
    let mut rng = Rng::new(0x5EED_2026 ^ (days as u64) << 32);

    let mut count = 0usize;
    for offset in (0..days).rev() {
        let date = today - ChronoDuration::days(offset);
        count += day(&tx, date, offset == 0, now_secs, &mut rng)?;
    }
    tx.commit()?;
    Ok(count)
}

fn day(
    conn: &Connection,
    date: NaiveDate,
    is_today: bool,
    now_secs: i64,
    rng: &mut Rng,
) -> Result<usize> {
    let weekend = date.weekday().num_days_from_monday() >= 5;
    if weekend && rng.chance(40) {
        return Ok(0);
    }

    let (start, mut end) = if weekend {
        let start = 9 * 3600 + rng.range(0, 3 * 3600) as i64;
        (start, (start + rng.range(2 * 3600, 8 * 3600) as i64).min(23 * 3600 + 30 * 60))
    } else {
        (
            8 * 3600 + 40 * 60 + rng.range(0, 4200) as i64,
            17 * 3600 + rng.range(0, 3 * 3600) as i64,
        )
    };
    if is_today {
        end = end.min(now_secs - 120);
    }

    let mut cursor = start;
    let mut last_app: Option<usize> = None;
    let mut lunch_done = false;
    let mut count = 0usize;

    while cursor < end {
        if !weekend && !lunch_done && cursor >= 11 * 3600 + 30 * 60 {
            lunch_done = true;
            if cursor < 13 * 3600 {
                cursor = 12 * 3600 + 40 * 60 + rng.range(0, 40 * 60) as i64;
                continue;
            }
        }

        let idx = pick_app(rng, weekend, last_app);
        last_app = Some(idx);
        let app = &APPS[idx];

        let span = (app.max_secs - app.min_secs) as f64;
        let dur = app.min_secs + (span * rng.float().powf(1.8)) as u64;
        let dur = dur.min((end - cursor) as u64);
        if dur < 5 {
            break;
        }

        let title = if rng.chance(4) {
            None
        } else {
            let pick = rng.range(0, app.titles.len() as u64 - 1) as usize;
            Some(app.titles[pick].to_string())
        };
        let started_at = at(date, cursor);
        let ended_at = started_at + (dur as i64) * 1000;

        let session = SessionStart {
            bundle_id: app.bundle_id.to_string(),
            app_name: app.name.to_string(),
            app_path: Some(app.path.to_string()),
            window_title: title,
            started_at,
            pid: None,
        };
        let id = db::insert_session(conn, &session)?;
        db::touch_session(conn, id, ended_at)?;
        count += 1;

        let mut gap = rng.range(10, 240) as i64;
        let roll = rng.float();
        if roll < 0.05 {
            gap += rng.range(10 * 60, 50 * 60) as i64;
        } else if roll < 0.12 {
            gap += rng.range(3 * 60, 12 * 60) as i64;
        }
        cursor += dur as i64 + gap;
    }

    Ok(count)
}

fn at(date: NaiveDate, secs: i64) -> i64 {
    db::day_start_ms(date) + secs * 1000
}

fn pick_app(rng: &mut Rng, weekend: bool, avoid: Option<usize>) -> usize {
    let weight = |i: usize, app: &AppProfile| {
        let base = if weekend { app.weekend_weight } else { app.weight };
        if avoid == Some(i) { (base / 2).max(1) } else { base }
    };
    let total: u32 = APPS
        .iter()
        .enumerate()
        .map(|(i, app)| weight(i, app))
        .sum();
    let mut roll = rng.range(1, total as u64) as u32;
    for (i, app) in APPS.iter().enumerate() {
        let w = weight(i, app);
        if roll <= w {
            return i;
        }
        roll -= w;
    }
    APPS.len() - 1
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next() % (hi - lo + 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.range(1, 100) <= percent
    }

    fn float(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}
