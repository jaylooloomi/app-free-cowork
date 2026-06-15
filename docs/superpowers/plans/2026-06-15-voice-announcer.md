# 任務完成語音播報 實作計畫

> 對應規格:`docs/superpowers/specs/2026-06-15-task-completion-voice-announcer-design.md`
> 風險已用並行 workflow 查證(Tauri overlay 焦點、WebView2 autoplay、摘要來源)。

**Goal:** 任務完成時,在焦點螢幕下方中央浮出玻璃 overlay,逐句顯示並語音朗讀一段摘要,唸完淡出。

**Architecture:** 新增 `announcer` 視窗(透明置頂、`WS_EX_NOACTIVATE` 不搶焦點、點擊穿透);`on_done` 完成時從 stream-json log 解析出 Claude Code 的 `result` 摘要 → 顯示視窗 + emit 文字 → 前端 Web Speech 朗讀。

**Tech Stack:** Tauri 2 / Rust(`windows` crate Win32)、SvelteKit、Web Speech API、`window-vibrancy`。

---

## 可行性評估:🟢 可行(兩個風險皆有已驗證的緩解)

| 風險 | 結論 | 緩解 |
|---|---|---|
| 摘要要不要再呼叫模型 | 不用 | 解析 log 內既有的 `result` 事件(Claude Code 自帶摘要),fallback 最後 assistant 文字→靜態句 |
| overlay 搶焦點(打斷打字) | Tauri flag 不可靠 | `WS_EX_NOACTIVATE` via `hwnd()`+`SetWindowLongPtrW`(features 已具備),show 前套用 |
| WebView2 不自動發聲 | 預設會擋 | 視窗 `additionalBrowserArgs` 加 `--autoplay-policy=no-user-gesture-required`(保留 wry 預設)+ 首次手勢 warm-up |
| 語音清單 | 無 Google 音 | 動態 `getVoices()` 按 `lang==='zh-TW'` 挑(HsiaoChen/Hanhan),處理 async/空清單 |

## 檔案異動

- `src-tauri/src/settings.rs` — 加 `announce_enabled`(預設 true)、`announce_voice`(預設 "")
- `src-tauri/src/announce.rs`(新)— 純函式 `extract_summary(log: &str) -> Option<String>` + `bottom_centered_position(...)`
- `src-tauri/src/lib.rs` — `show_announcer`、套 fx、`WS_EX_NOACTIVATE`、`announcer_done` 命令、setup 註冊
- `src-tauri/src/ipc.rs` — `on_done`/`handle_task_exit` 完成時觸發(若 `announce_enabled`)
- `src-tauri/tauri.conf.json` — 新增 `announcer` 視窗 + 各視窗 `additionalBrowserArgs`
- `src/lib/Announcer.svelte`(新)、`src/routes/+page.svelte`(加 branch)、`src/lib/Settings.svelte`(開關+語音下拉)、`src/lib/api.ts`(型別)

## 任務(TDD;純邏輯先測)

### Task 1 — Settings 欄位
`settings.rs` struct + Default 加兩欄(`#[serde(default)]` 自動相容舊檔)。測試:default 為 true/""、partial JSON 保留預設。

### Task 2 — `extract_summary`(純函式,核心)
`announce.rs`:逐行 parse stream-json,取**最後一個** `{"type":"result"}` 的文字(否則最後 `post_turn_summary`,否則最後 assistant 文字),截到 ~2 句。測試:多種 JSONL 樣本(有 result / 只有 assistant / 空 / 非 JSON)→ 期望輸出。

### Task 3 — `bottom_centered_position`(純函式)
`announce.rs`:X 與 palette 相同置中;Y = `monitor_pos.y + (height - win_h - margin)`(saturating)。測試:主/副螢幕、太高視窗 clamp。

### Task 4 — `announcer` 視窗設定
`tauri.conf.json` 加視窗:`transparent/decorations:false/alwaysOnTop/skipTaskbar/visible:false/focus:false/resizable:false`,`width:560 height:200`。**所有視窗**加 `additionalBrowserArgs:"--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required"`(因 WebView2 環境跨視窗共用,需一致)。

### Task 5 — 前端 Announcer.svelte + 路由
`Announcer.svelte`(沿用 demo 視覺):onMount 監聽 `announce` 事件→切句→逐句淡入+Web Speech(動態挑 zh-TW 語音,用 `settings.announce_voice` 優先)+音波→唸完 invoke `announcer_done`。首次以空 utterance warm-up。`+page.svelte` 加 `{:else if label==="announcer"}` branch(在 `{:else}` 之前)。

### Task 6 — Rust show_announcer + 不搶焦點 + 命令
`lib.rs`:`show_announcer(app, text)` → `bottom_centered_position` 定位 → `hwnd()` 套 `WS_EX_NOACTIVATE` → `set_ignore_cursor_events(true)` → `show()`(不呼叫 set_focus)→ emit `announce` 帶 text。`announcer_done` 命令 hide 視窗。setup() 對 announcer 套 `fx::apply_palette_effects`。

### Task 7 — 接 on_done
`ipc.rs` `handle_task_exit`(背景 + code==0 那條,toast 旁)或 `on_done`:若 `announce_enabled`,讀 log(完整檔)→ `extract_summary` → `show_announcer`(另起 thread 避免擋佇列)。失敗 fallback 靜態句。

### Task 8 — 設定 UI
`Settings.svelte` 加「語音播報」開關(綁 `announce_enabled`)+ 語音下拉(`getVoices()` 過濾 zh,綁 `announce_voice`)。`api.ts` 補型別。

### Task 9 — 建置與驗證
`npm --prefix launcher run tauri build -- --no-bundle` → 覆蓋安裝 → 重啟 → 手動驗證:跑一個任務,overlay 在焦點螢幕下方出現、不搶焦點(打字不中斷)、語音朗讀、唸完淡出;設定開關有效。

## 範圍外(未來):雲端 TTS、真實音波、自訂語速/秒數、歷史重聽。
