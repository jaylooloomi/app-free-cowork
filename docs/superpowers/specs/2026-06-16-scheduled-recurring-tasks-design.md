# 設計規格:週期性排程任務(Scheduled / Recurring Tasks)

狀態:設計定稿、**尚未實作**(使用者要求先規劃)。
日期:2026-06-16

## 目標

讓使用者透過輸入面板的 🕐 排程按鈕,把一個提示詞設成**週期性自動執行**(例如每天 09:00 跑、
每 30 分鐘跑一次),由 app 內建排程器在背景按時觸發,走現有任務佇列執行,並可隨時暫停 / 刪除。

## 核心決策(已與使用者確認)

| 項目 | 決定 |
|---|---|
| 排程引擎 | **app 內建背景執行緒**,**不用** Windows Task Scheduler(自包含、免管理員、UI 內可管理) |
| 週期模型 | **簡單預設**:每 N 分鐘/小時、每天某時刻、每週某天某時刻 |
| 送出時 | 立即跑一次 **+** 之後週期跑(預設;週期面板可勾「只排程不立刻跑」) |
| app 關閉時錯過的執行 | **跳過**,下次到期再跑(不補跑) |
| 到期時已有任務在跑 | **排進現有佇列**(沿用現有行為) |
| 取消 | 佇列面板「排程中」區:**暫停/啟用** 切換 + **刪除**;刪除只停未來,不中止正在跑的那次 |

> 取捨:in-app 排程只在 app 執行中才會觸發。app 本來就開機自啟(`autostart`)+ 常駐系統匣,
> 故實務上一直在跑。若日後要「app 沒開也能準時跑」,再加 Windows Task Scheduler
> (用既有的 `launcher.exe --run <prompt>`)即可,與本設計不衝突。

## 週期模型

```
enum Recurrence {
  EveryMinutes(u32),          // 每 N 分鐘
  EveryHours(u32),            // 每 N 小時
  DailyAt { hour, minute },   // 每天 HH:MM
  WeeklyAt { weekday, hour, minute },  // 每週某天 HH:MM
}
```

**`next_run(recurrence, last_or_now) -> DateTime`** 為純函式(排程正確性的核心,單元測試重點):
- `EveryMinutes/Hours`:`last_run + 間隔`(首次為 now + 間隔,或 now 視「立即跑」而定)。
- `DailyAt`:今天該時刻若已過 → 明天該時刻。
- `WeeklyAt`:本週該星期該時刻若已過 → 下週。
- 用 `chrono`(專案已相依)做時間計算,以**本地時間**為準。

## 資料模型與儲存

`schedules.json`(放 `dirs::config_dir()/free-claude-code/schedules.json`,與 settings 同處):

```
struct Schedule {
  id: u64,
  prompt: String,
  workdir: Option<String>,
  recurrence: Recurrence,
  run_immediately: bool,   // 建立時是否立刻跑一次
  enabled: bool,           // 暫停 = false
  next_run: i64,           // unix 秒;重啟時重算
  last_run: Option<i64>,
}
```

- 儲存/載入比照 `settings.rs`(`serde` + 原子寫入 `.tmp` → rename)。
- 啟動時載入並**重算 `next_run`**(避免用過期值)。

## 架構與資料流

```
按 🕐 → 前端彈出週期選擇器(類型 + 參數)→ 輸入提示詞 → 送出
  → IPC create_schedule(prompt, workdir, recurrence, run_immediately)
       → 寫入 schedules.json + 記憶體清單(Mutex 保護)
       → run_immediately ? 立刻走 submit 路徑跑一次
  ── 背景排程器執行緒(每 30 秒 tick)──
       → 掃描 enabled 且 now >= next_run 的排程
       → 對每筆:走現有 do_launch / submit 佇列路徑執行(沿用佇列、on_done、語音播報)
       → 更新 last_run、重算 next_run、存檔
```

- **排程器執行緒**:在 `setup()` 內 `std::thread::spawn`(仿 `refresh_catalog` 的背景模式),
  持 `AppHandle`,迴圈 `sleep(30s)` → 檢查 → 觸發。
- **執行路徑重用**:排程觸發等同於使用者送出一個提示詞 → 直接呼叫現有的提交/佇列邏輯,
  不另寫一套執行器。

## IPC 命令(比照現有 `queue_*` / `dismiss_completed` 模式)

- `create_schedule(prompt, workdir, recurrence, run_immediately) -> id`
- `list_schedules() -> Vec<ScheduleDto>`(含人類可讀的週期字串與 next_run)
- `delete_schedule(id)`
- `set_schedule_enabled(id, enabled)`

排程清單以 `Mutex` 保護(排程器執行緒與 IPC 命令共用)。

## UI

- **🕐 排程按鈕**(已加,目前無功能):點擊切換「排程模式」並展開週期選擇器(週期類型下拉 +
  參數欄位 + 「只排程不立刻跑」勾選)。🕐 高亮表示本次送出為排程。再點一次取消排程模式。
- **送出**:排程模式下,`submit()` 改呼叫 `create_schedule` 而非 `submit_prompt`。
- **「排程中」區**(佇列面板內):每筆顯示 提示詞(截斷)+ 週期 +「下次:…」+ ⏸暫停/▶啟用 + ✕刪除。

## 錯誤處理

- `schedules.json` 損毀 → 備份 `.bak` + 視為空清單(比照 settings 的降級)。
- 排程觸發時 app 處於需設定/未登入 → 比照 `submit_prompt` 既有處理(通知/精靈),該次略過。
- 排程器執行緒內所有鎖用 poison-tolerant 取法,單筆排程失敗不影響其他與迴圈本身。

## 測試

- **純函式單元測試**:`next_run` —— 每種週期 × 邊界(剛好到點、已過、跨日/跨週、N=0 防呆)。
- **儲存**:`schedules.json` 載入/儲存/部分欄位相容/損毀降級。
- **手動驗證**:建立各類週期排程 → 確認到期觸發、立即跑、暫停/啟用、刪除、重啟後仍在且 next_run 正確。

## 範圍(YAGNI)

**v1**:簡單預設週期、in-app 排程器、立即跑、暫停/刪除、重啟存活、佇列面板管理區。

**未來(不做)**:cron 表達式、補跑錯過的執行、Windows Task Scheduler(app 沒開也跑)、
就地編輯排程、多時刻/多星期、結束日期/次數上限。

## 重用既有資產

現有任務佇列與 `do_launch`/`submit_prompt`、`on_done`、語音播報、`settings.rs` 的儲存模式、
`chrono` 相依、佇列面板 UI 與 `queue_*` 命令模式、背景執行緒模式(`refresh_catalog`)。
