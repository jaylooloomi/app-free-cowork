//! 週期性排程:週期模型、下次執行時間(純函式,單元測試核心)、schedules.json 持久化。

use chrono::{DateTime, Datelike, Duration, Local, TimeZone};
use serde::{Deserialize, Serialize};

/// 週期模型。具名欄位變體 → serde tag 模式序列化乾淨(`{"kind":"every_minutes","minutes":30}`),
/// 前端好對應。weekday:Mon=1 .. Sun=7。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recurrence {
    EveryMinutes { minutes: u32 },
    EveryHours { hours: u32 },
    DailyAt { hour: u32, minute: u32 },
    WeeklyAt { weekday: u32, hour: u32, minute: u32 },
}

/// 下次執行時間(本地時區)。間隔型:now + 間隔(0 視為 1);每日/每週:該時刻若已過則順延。
pub fn next_run(r: &Recurrence, now: DateTime<Local>) -> DateTime<Local> {
    match r {
        Recurrence::EveryMinutes { minutes } => now + Duration::minutes((*minutes).max(1) as i64),
        Recurrence::EveryHours { hours } => now + Duration::hours((*hours).max(1) as i64),
        Recurrence::DailyAt { hour, minute } => {
            let today = local_at(now, *hour, *minute);
            if today > now {
                today
            } else {
                today + Duration::days(1)
            }
        }
        Recurrence::WeeklyAt { weekday, hour, minute } => {
            let target = (*weekday).clamp(1, 7) as i64;
            let cur = now.weekday().number_from_monday() as i64; // Mon=1 .. Sun=7
            let days = (target - cur).rem_euclid(7);
            let at = local_at(now, *hour, *minute);
            let candidate = at + Duration::days(days);
            if candidate > now {
                candidate
            } else {
                at + Duration::days(days + 7)
            }
        }
    }
}

/// 今天的某時刻(本地)。
fn local_at(now: DateTime<Local>, hour: u32, minute: u32) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), hour.min(23), minute.min(59), 0)
        .single()
        .unwrap_or(now)
}

/// 一筆週期排程。`#[serde(default)]` 讓未來新增欄位時舊 schedules.json 仍可載入。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: u64,
    pub prompt: String,
    pub workdir: Option<String>,
    pub recurrence: Recurrence,
    #[serde(default)]
    pub run_immediately: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub next_run: i64, // unix 秒
    #[serde(default)]
    pub last_run: Option<i64>,
}
fn yes() -> bool {
    true
}

/// 載入排程清單。檔案不存在 → 空;損毀 → 備份成 .bak 並回空(比照 settings 降級)。
pub fn load(path: &std::path::Path) -> Vec<Schedule> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<Schedule>>(&s) {
        Ok(v) => v,
        Err(_) => {
            let _ = std::fs::rename(path, path.with_extension("json.bak"));
            Vec::new()
        }
    }
}

/// 原子寫入(.tmp → rename),比照 settings::save。
pub fn save(path: &std::path::Path, list: &[Schedule]) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(list).unwrap().as_bytes())?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike; // 測試用 .hour()/.minute()

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn every_minutes_adds_interval() {
        assert_eq!(
            next_run(&Recurrence::EveryMinutes { minutes: 30 }, dt(2026, 6, 16, 9, 0)),
            dt(2026, 6, 16, 9, 30)
        );
    }

    #[test]
    fn every_hours_adds_interval() {
        assert_eq!(
            next_run(&Recurrence::EveryHours { hours: 2 }, dt(2026, 6, 16, 9, 0)),
            dt(2026, 6, 16, 11, 0)
        );
    }

    #[test]
    fn daily_today_if_future_else_tomorrow() {
        assert_eq!(
            next_run(&Recurrence::DailyAt { hour: 9, minute: 0 }, dt(2026, 6, 16, 8, 0)),
            dt(2026, 6, 16, 9, 0)
        );
        assert_eq!(
            next_run(&Recurrence::DailyAt { hour: 9, minute: 0 }, dt(2026, 6, 16, 9, 30)),
            dt(2026, 6, 17, 9, 0)
        );
    }

    #[test]
    fn weekly_advances_to_target_weekday() {
        // 2026-06-16 是週二;目標週一 09:00 → 下週一 2026-06-22 09:00
        let r = Recurrence::WeeklyAt { weekday: 1, hour: 9, minute: 0 };
        let n = next_run(&r, dt(2026, 6, 16, 10, 0));
        assert_eq!((n.year(), n.month(), n.day(), n.hour(), n.minute()), (2026, 6, 22, 9, 0));
    }

    #[test]
    fn weekly_same_day_future_time_is_today() {
        // 週二 08:00,目標週二 09:00 → 今天(同日)09:00
        let r = Recurrence::WeeklyAt { weekday: 2, hour: 9, minute: 0 };
        let n = next_run(&r, dt(2026, 6, 16, 8, 0));
        assert_eq!((n.year(), n.month(), n.day(), n.hour()), (2026, 6, 16, 9));
    }

    #[test]
    fn zero_interval_is_clamped_to_one() {
        assert_eq!(
            next_run(&Recurrence::EveryMinutes { minutes: 0 }, dt(2026, 6, 16, 9, 0)),
            dt(2026, 6, 16, 9, 1)
        );
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("schedules.json");
        let list = vec![Schedule {
            id: 1,
            prompt: "整理桌面".into(),
            workdir: None,
            recurrence: Recurrence::DailyAt { hour: 9, minute: 0 },
            run_immediately: true,
            enabled: true,
            next_run: 123,
            last_run: None,
        }];
        save(&p, &list).unwrap();
        assert_eq!(load(&p), list);
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("nope.json")).is_empty());
    }

    #[test]
    fn load_corrupt_returns_empty_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("schedules.json");
        std::fs::write(&p, "{bad").unwrap();
        assert!(load(&p).is_empty());
        assert!(p.with_extension("json.bak").exists());
    }
}
