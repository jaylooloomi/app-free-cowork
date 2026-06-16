# 週期性排程任務 實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development 或 superpowers:executing-plans 逐任務實作。步驟用 `- [ ]` 追蹤。
> 對應規格:`docs/superpowers/specs/2026-06-16-scheduled-recurring-tasks-design.md`

**Goal:** 讓使用者把提示詞設成週期性自動執行,由 app 內建排程器在背景按時觸發、走現有任務佇列,並可暫停/刪除。

**Architecture:** 純函式 `next_run` + `Schedule`/`ScheduleStore`(`schedule.rs`)負責週期計算與持久化;`AppState` 持有記憶體排程清單;`setup()` 起一條背景執行緒每 30s 檢查到期排程,走現有 `submit_prompt` 佇列路徑執行;前端 🕐 切換排程模式 + 週期選擇器 + 「排程中」管理區。

**Tech Stack:** Rust / Tauri 2、`chrono`(時間)、`serde`(持久化)、SvelteKit 前端。

---

## 檔案結構

- **建立** `launcher/src-tauri/src/schedule.rs` — `Recurrence` enum、`next_run` 純函式、`Schedule` struct、`load`/`save`(schedules.json)。純邏輯 + 持久化,單元測試集中於此。
- **修改** `launcher/src-tauri/src/ipc.rs` — `AppState` 加 `schedules` 欄位;4 個 IPC 命令;排程觸發走現有提交路徑的小封裝。
- **修改** `launcher/src-tauri/src/lib.rs` — `schedules_path()`;`setup()` 載入排程 + 起排程器執行緒;註冊命令。
- **修改** `launcher/src-tauri/src/main.rs` 無需動(走 lib)。
- **修改** 前端 `src/lib/api.ts`(型別 + 命令)、`src/lib/Palette.svelte`(排程模式 UI + 送出分支 + 排程中清單)。

---

## Task 1：Recurrence + next_run(純函式)

**Files:** Create `launcher/src-tauri/src/schedule.rs`;Modify `launcher/src-tauri/src/lib.rs`(加 `pub mod schedule;`)

- [ ] **Step 1: 寫失敗測試**

```rust
// schedule.rs 底部
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Local, Datelike, Timelike};

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn every_minutes_adds_interval() {
        let now = dt(2026, 6, 16, 9, 0);
        assert_eq!(next_run(&Recurrence::EveryMinutes(30), now), dt(2026, 6, 16, 9, 30));
    }

    #[test]
    fn daily_today_if_future_else_tomorrow() {
        // 現在 08:00,DailyAt 09:00 → 今天 09:00
        assert_eq!(next_run(&Recurrence::DailyAt { hour: 9, minute: 0 }, dt(2026,6,16,8,0)), dt(2026,6,16,9,0));
        // 現在 09:30,DailyAt 09:00 → 明天 09:00
        assert_eq!(next_run(&Recurrence::DailyAt { hour: 9, minute: 0 }, dt(2026,6,16,9,30)), dt(2026,6,17,9,0));
    }

    #[test]
    fn weekly_advances_to_target_weekday() {
        // 2026-06-16 是週二(weekday Tue);WeeklyAt 週一 09:00 → 下週一 2026-06-22 09:00
        let r = Recurrence::WeeklyAt { weekday: 1, hour: 9, minute: 0 }; // 1 = Monday (Mon=1..Sun=7)
        let n = next_run(&r, dt(2026,6,16,10,0));
        assert_eq!((n.year(), n.month(), n.day(), n.hour(), n.minute()), (2026, 6, 22, 9, 0));
    }

    #[test]
    fn zero_interval_is_clamped_to_one() {
        let now = dt(2026,6,16,9,0);
        assert_eq!(next_run(&Recurrence::EveryMinutes(0), now), dt(2026,6,16,9,1));
    }
}
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `cargo test --release --lib --manifest-path launcher/src-tauri/Cargo.toml schedule::`
Expected: 編譯錯(`Recurrence`/`next_run` 未定義)。

- [ ] **Step 3: 寫最小實作**

```rust
// schedule.rs 頂部
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recurrence {
    EveryMinutes(u32),
    EveryHours(u32),
    DailyAt { hour: u32, minute: u32 },
    WeeklyAt { weekday: u32, hour: u32, minute: u32 }, // weekday: Mon=1 .. Sun=7
}

/// 下次執行時間(本地時區)。間隔型:now + 間隔(0 視為 1);每日/每週:該時刻若已過則順延。
pub fn next_run(r: &Recurrence, now: DateTime<Local>) -> DateTime<Local> {
    match r {
        Recurrence::EveryMinutes(n) => now + Duration::minutes((*n).max(1) as i64),
        Recurrence::EveryHours(n) => now + Duration::hours((*n).max(1) as i64),
        Recurrence::DailyAt { hour, minute } => {
            let today = local_at(now, *hour, *minute);
            if today > now { today } else { today + Duration::days(1) }
        }
        Recurrence::WeeklyAt { weekday, hour, minute } => {
            let target = (*weekday).clamp(1, 7) as i64;
            let cur = now.weekday().number_from_monday() as i64; // Mon=1..Sun=7
            let mut days = (target - cur).rem_euclid(7);
            let candidate = local_at(now, *hour, *minute) + Duration::days(days);
            if candidate <= now {
                days += 7;
                local_at(now, *hour, *minute) + Duration::days(days)
            } else {
                candidate
            }
        }
    }
}

fn local_at(now: DateTime<Local>, hour: u32, minute: u32) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(now.year(), now.month(), now.day(), hour.min(23), minute.min(59), 0)
        .single()
        .unwrap_or(now)
}
```
並在 `lib.rs` 模組宣告區加入 `pub mod schedule;`。

- [ ] **Step 4: 跑測試確認通過**

Run: `cargo test --release --lib --manifest-path launcher/src-tauri/Cargo.toml schedule::`
Expected: PASS(4 tests）。

- [ ] **Step 5: Commit**

```bash
git add launcher/src-tauri/src/schedule.rs launcher/src-tauri/src/lib.rs
git commit -m "feat(schedule): recurrence model + next_run pure fn"
```

---

## Task 2：Schedule struct + 持久化(load/save）

**Files:** Modify `launcher/src-tauri/src/schedule.rs`

- [ ] **Step 1: 寫失敗測試**

```rust
// 加進 schedule.rs 的 tests mod
#[test]
fn save_then_load_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("schedules.json");
    let list = vec![Schedule {
        id: 1, prompt: "整理桌面".into(), workdir: None,
        recurrence: Recurrence::DailyAt { hour: 9, minute: 0 },
        run_immediately: true, enabled: true, next_run: 123, last_run: None,
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
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `cargo test --release --lib --manifest-path launcher/src-tauri/Cargo.toml schedule::`
Expected: 編譯錯(`Schedule`/`load`/`save` 未定義)。

- [ ] **Step 3: 寫最小實作**

```rust
// schedule.rs
use std::io::Write as _;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: u64,
    pub prompt: String,
    pub workdir: Option<String>,
    pub recurrence: Recurrence,
    #[serde(default)] pub run_immediately: bool,
    #[serde(default = "yes")] pub enabled: bool,
    #[serde(default)] pub next_run: i64,   // unix 秒
    #[serde(default)] pub last_run: Option<i64>,
}
fn yes() -> bool { true }

pub fn load(path: &Path) -> Vec<Schedule> {
    let Ok(s) = std::fs::read_to_string(path) else { return Vec::new() };
    match serde_json::from_str::<Vec<Schedule>>(&s) {
        Ok(v) => v,
        Err(_) => {
            let _ = std::fs::rename(path, path.with_extension("json.bak"));
            Vec::new()
        }
    }
}

pub fn save(path: &Path, list: &[Schedule]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string_pretty(list).unwrap().as_bytes())?;
    std::fs::rename(&tmp, path)
}
```

- [ ] **Step 4: 跑測試確認通過**

Run: `cargo test --release --lib --manifest-path launcher/src-tauri/Cargo.toml schedule::`
Expected: PASS(全部 7 tests）。

- [ ] **Step 5: Commit**

```bash
git add launcher/src-tauri/src/schedule.rs
git commit -m "feat(schedule): Schedule struct + json load/save"
```

---

## Task 3：AppState 整合 + schedules_path + 啟動載入

**Files:** Modify `launcher/src-tauri/src/ipc.rs`(`AppState`)、`launcher/src-tauri/src/lib.rs`

- [ ] **Step 1: AppState 加欄位**

在 `ipc.rs` 的 `AppState`(找 `pub struct AppState`)加:
```rust
pub schedules: std::sync::Mutex<Vec<crate::schedule::Schedule>>,
pub next_schedule_id: std::sync::atomic::AtomicU64,
```
在 `AppState::new(...)` 初始化:
```rust
schedules: std::sync::Mutex::new(Vec::new()),
next_schedule_id: std::sync::atomic::AtomicU64::new(1),
```

- [ ] **Step 2: schedules_path 輔助函式**(`lib.rs`,仿 `settings::settings_path`)

```rust
pub fn schedules_path() -> std::path::PathBuf {
    crate::logging::logs_dir().parent().map(|d| d.join("schedules.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("schedules.json"))
}
```
(`logs_dir()` = `config_dir/free-claude-code/logs`,其 parent 即 `config_dir/free-claude-code`。)

- [ ] **Step 3: setup() 啟動載入 + 重算 next_run**

在 `lib.rs` `setup()`(`build_tray` 之後、`handle_argv` 之前)加:
```rust
{
    let mut list = schedule::load(&schedules_path());
    let now = chrono::Local::now();
    for s in list.iter_mut() {
        s.next_run = schedule::next_run(&s.recurrence, now).timestamp();
    }
    let st = handle.state::<AppState>();
    let max = list.iter().map(|s| s.id).max().unwrap_or(0);
    st.next_schedule_id.store(max + 1, std::sync::atomic::Ordering::Relaxed);
    *st.schedules.lock().unwrap() = list;
}
```

- [ ] **Step 4: 編譯驗證**

Run: `cargo build --release --manifest-path launcher/src-tauri/Cargo.toml`
Expected: 編過(無功能變更,僅整合)。

- [ ] **Step 5: Commit**

```bash
git add launcher/src-tauri/src/ipc.rs launcher/src-tauri/src/lib.rs
git commit -m "feat(schedule): AppState integration + load on startup"
```

---

## Task 4：IPC 命令(create/list/delete/enable)

**Files:** Modify `launcher/src-tauri/src/ipc.rs`、`launcher/src-tauri/src/lib.rs`(註冊)

- [ ] **Step 1: 在 ipc.rs 加命令**

```rust
#[derive(serde::Serialize)]
pub struct ScheduleDto { pub id: u64, pub prompt: String, pub recurrence: crate::schedule::Recurrence, pub enabled: bool, pub next_run: i64 }

#[tauri::command]
pub fn create_schedule(app: AppHandle, state: State<AppState>, prompt: String, workdir: Option<String>, recurrence: crate::schedule::Recurrence, run_immediately: bool) -> u64 {
    let id = state.next_schedule_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = chrono::Local::now();
    let s = crate::schedule::Schedule {
        id, prompt: prompt.clone(), workdir: workdir.clone(), recurrence: recurrence.clone(),
        run_immediately, enabled: true,
        next_run: crate::schedule::next_run(&recurrence, now).timestamp(), last_run: None,
    };
    { let mut g = state.schedules.lock_safe(); g.push(s); persist(&g); }
    if run_immediately { trigger_prompt(&app, prompt, workdir); }
    id
}

#[tauri::command]
pub fn list_schedules(state: State<AppState>) -> Vec<ScheduleDto> {
    state.schedules.lock_safe().iter().map(|s| ScheduleDto {
        id: s.id, prompt: s.prompt.clone(), recurrence: s.recurrence.clone(), enabled: s.enabled, next_run: s.next_run,
    }).collect()
}

#[tauri::command]
pub fn delete_schedule(state: State<AppState>, id: u64) {
    let mut g = state.schedules.lock_safe();
    g.retain(|s| s.id != id);
    persist(&g);
}

#[tauri::command]
pub fn set_schedule_enabled(state: State<AppState>, id: u64, enabled: bool) {
    let mut g = state.schedules.lock_safe();
    if let Some(s) = g.iter_mut().find(|s| s.id == id) { s.enabled = enabled; }
    persist(&g);
}

fn persist(list: &[crate::schedule::Schedule]) {
    let _ = crate::schedule::save(&crate::schedules_path(), list);
}

/// 排程觸發 = 等同使用者送出一個提示詞:走現有提交路徑(背景執行)。
fn trigger_prompt(app: &AppHandle, prompt: String, workdir: Option<String>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(e) = submit_prompt(app.clone(), state, prompt, workdir).await {
            crate::notify(&app, &e);
        }
    });
}
```
(`lock_safe()` 為專案既有的 poison-tolerant 取鎖;若 `AppState.schedules` 用 `std::sync::Mutex`,沿用 `lock_safe` 需確認 `LockExt` 適用,否則改 `.lock().unwrap_or_else(|e| e.into_inner())`。)

- [ ] **Step 2: lib.rs 註冊命令**

在 `tauri::generate_handler![ ... ]` 清單加:
```rust
ipc::create_schedule,
ipc::list_schedules,
ipc::delete_schedule,
ipc::set_schedule_enabled,
```

- [ ] **Step 3: 編譯驗證**

Run: `cargo build --release --manifest-path launcher/src-tauri/Cargo.toml`
Expected: 編過。

- [ ] **Step 4: Commit**

```bash
git add launcher/src-tauri/src/ipc.rs launcher/src-tauri/src/lib.rs
git commit -m "feat(schedule): IPC commands create/list/delete/enable"
```

---

## Task 5：背景排程器執行緒

**Files:** Modify `launcher/src-tauri/src/lib.rs`

- [ ] **Step 1: 加排程器函式並在 setup() 啟動**

```rust
fn start_scheduler(handle: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(30));
        let now = chrono::Local::now();
        let now_ts = now.timestamp();
        // 收集到期者(複製出最小資訊後立即放鎖,避免在持鎖期間執行)
        let due: Vec<(u64, String, Option<String>)> = {
            let st = handle.state::<AppState>();
            let mut g = st.schedules.lock().unwrap_or_else(|e| e.into_inner());
            let mut due = Vec::new();
            for s in g.iter_mut() {
                if s.enabled && now_ts >= s.next_run {
                    due.push((s.id, s.prompt.clone(), s.workdir.clone()));
                    s.last_run = Some(now_ts);
                    s.next_run = schedule::next_run(&s.recurrence, now).timestamp();
                }
            }
            if !due.is_empty() { let _ = schedule::save(&schedules_path(), &g); }
            due
        };
        for (_id, prompt, workdir) in due {
            ipc::trigger_prompt(&handle, prompt, workdir); // 走現有佇列;到期時若有任務在跑會自動排隊
        }
    });
}
```
在 `setup()` Task 3 載入排程之後呼叫 `start_scheduler(handle.clone());`。
(`ipc::trigger_prompt` 需從 Task 4 的 `fn` 改為 `pub(crate) fn`。)

- [ ] **Step 2: 編譯驗證**

Run: `cargo build --release --manifest-path launcher/src-tauri/Cargo.toml`
Expected: 編過。

- [ ] **Step 3: Commit**

```bash
git add launcher/src-tauri/src/lib.rs launcher/src-tauri/src/ipc.rs
git commit -m "feat(schedule): background scheduler thread (30s tick)"
```

---

## Task 6：前端 api.ts(型別 + 命令)

**Files:** Modify `launcher/src/lib/api.ts`

- [ ] **Step 1: 加型別與命令**

```typescript
export type Recurrence =
  | { kind: "every_minutes"; 0: number }       // 注意:tag+u32 變體序列化見後註
  | { kind: "every_hours"; 0: number }
  | { kind: "daily_at"; hour: number; minute: number }
  | { kind: "weekly_at"; weekday: number; hour: number; minute: number };

export interface ScheduleDto { id: number; prompt: string; recurrence: Recurrence; enabled: boolean; next_run: number }
```
在 `api` 物件加:
```typescript
createSchedule: (prompt: string, workdir: string | null, recurrence: Recurrence, runImmediately: boolean) =>
  invoke<number>("create_schedule", { prompt, workdir, recurrence, runImmediately }),
listSchedules: () => invoke<ScheduleDto[]>("list_schedules"),
deleteSchedule: (id: number) => invoke<void>("delete_schedule", { id }),
setScheduleEnabled: (id: number, enabled: boolean) => invoke<void>("set_schedule_enabled", { id, enabled }),
```
> 註:`Recurrence` 用 serde `#[serde(tag="kind")]`。`EveryMinutes(u32)` 這種 tuple 變體在 tag 模式下序列化為 `{"kind":"every_minutes", "0": 30}` 不直觀;**實作時把 `EveryMinutes`/`EveryHours` 也改成具名欄位** `EveryMinutes { minutes: u32 }` / `EveryHours { hours: u32 }`,前端型別對應 `{kind:"every_minutes", minutes:number}`。(同步回頭調整 Task 1 的 enum 與測試。)

- [ ] **Step 2: 型別檢查**

Run: `npm --prefix launcher run check`
Expected: 0 errors。

- [ ] **Step 3: Commit**

```bash
git add launcher/src/lib/api.ts
git commit -m "feat(schedule): frontend api types + commands"
```

---

## Task 7：Palette 排程模式(🕐 切換 + 週期選擇器 + 送出分支)

**Files:** Modify `launcher/src/lib/Palette.svelte`

- [ ] **Step 1: 狀態 + 🕐 onclick**

在 `<script>` 加:
```typescript
let scheduleMode = $state(false);
let recKind = $state<"every_minutes"|"every_hours"|"daily_at"|"weekly_at">("daily_at");
let recEvery = $state(30);     // 分鐘或小時數
let recHour = $state(9);
let recMinute = $state(0);
let recWeekday = $state(1);
let runImmediately = $state(true);

function buildRecurrence(): Recurrence {
  if (recKind === "every_minutes") return { kind: "every_minutes", minutes: recEvery } as any;
  if (recKind === "every_hours")   return { kind: "every_hours", hours: recEvery } as any;
  if (recKind === "daily_at")      return { kind: "daily_at", hour: recHour, minute: recMinute };
  return { kind: "weekly_at", weekday: recWeekday, hour: recHour, minute: recMinute };
}
```
把 Task(時鐘按鈕)那顆 `<button class="sched">` 加上 `onclick={() => (scheduleMode = !scheduleMode)}` 與 `class:active={scheduleMode}`。

- [ ] **Step 2: 週期選擇器(輸入框上方,排程模式才顯示)**

在輸入列上方加(`{#if scheduleMode}` 區塊):週期類型下拉(每分/每時/每天/每週)、對應參數(數字 input / 時:分 / 星期下拉)、「立即跑一次」勾選 `bind:checked={runImmediately}`。皆 `bind:` 到上面狀態。

- [ ] **Step 3: 送出分支**

在 `submit()`(`Palette.svelte:230`)起始處加:
```typescript
if (scheduleMode) {
  await api.createSchedule(fullPrompt, workdir, buildRecurrence(), runImmediately);
  scheduleMode = false;
  input = "";
  return; // 不走 submitPrompt
}
```
(`fullPrompt`/`workdir` 沿用現有 submit 內既有變數。)

- [ ] **Step 4: 型別檢查 + 手動驗證**

Run: `npm --prefix launcher run check` → 0 errors。
手動:🕐 切換 → 選每分鐘 1 → 輸入「ping」→ 送出 → 約 1 分鐘後應自動執行一次。

- [ ] **Step 5: Commit**

```bash
git add launcher/src/lib/Palette.svelte
git commit -m "feat(schedule): palette schedule mode + recurrence picker"
```

---

## Task 8：「排程中」管理區(列出 / 暫停 / 刪除)

**Files:** Modify `launcher/src/lib/Palette.svelte`

- [ ] **Step 1: 載入 + 顯示清單**

`onMount` 內 `listSchedules()` 載入到 `let schedules = $state<ScheduleDto[]>([])`;在佇列/完成面板區塊下方加「排程中」清單:每列顯示 `prompt`(截斷)、人類可讀週期(寫一個 `recurrenceLabel(r)`)、下次執行(`new Date(next_run*1000).toLocaleString()`)。

- [ ] **Step 2: 暫停 / 刪除**

每列加:`⏸/▶` 按鈕 `onclick={() => api.setScheduleEnabled(s.id, !s.enabled).then(reloadSchedules)}`;`✕` 按鈕 `onclick={() => api.deleteSchedule(s.id).then(reloadSchedules)}`。`reloadSchedules()` 重新 `listSchedules()`。

- [ ] **Step 3: 型別檢查 + 手動驗證**

Run: `npm --prefix launcher run check` → 0 errors。
手動:建立排程 → 清單出現 → 暫停(不再觸發)→ 啟用 → 刪除(消失、不再觸發)。

- [ ] **Step 4: Commit**

```bash
git add launcher/src/lib/Palette.svelte
git commit -m "feat(schedule): scheduled-list management (pause/delete)"
```

---

## Task 9：建置、安裝、整體驗證

- [ ] **Step 1:** `npm --prefix launcher run tauri build -- --no-bundle`(opt-level 1 已在 Cargo.toml),覆蓋安裝 `%LOCALAPPDATA%\FreeCowork\launcher.exe`、用 `explorer.exe` 重啟。
- [ ] **Step 2:** 手動驗證:每分鐘排程會觸發、每天/每週 next_run 正確、立即跑、暫停/啟用/刪除、**重啟 app 後排程仍在且 next_run 正確**、到期時若有任務在跑會排隊。
- [ ] **Step 3: Commit / 收尾**(推分支或合併)。

---

## 自我檢查(對照規格)
- 週期模型 ✓(Task 1)、持久化 ✓(Task 2)、啟動重算 ✓(Task 3)、建立/列出/刪除/暫停 ✓(Task 4)、in-app 排程器 30s ✓(Task 5)、UI 排程模式 ✓(Task 7)、管理/取消 ✓(Task 8)、立即跑 ✓(Task 4/7)、到期排隊=重用佇列 ✓(Task 5)、重啟存活 ✓(Task 3+9)。
- 已知需在實作時收斂的點:Task 6 的 serde tag 變體 → 把 `EveryMinutes/EveryHours` 改具名欄位(已在 Task 6 註明,需回改 Task 1 enum 與測試);`lock_safe` 對 `std::sync::Mutex` 是否適用,否則用 `into_inner` 取法。
