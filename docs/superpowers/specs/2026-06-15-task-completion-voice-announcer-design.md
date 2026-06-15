# 設計規格:任務完成語音播報(Voice Announcer)

狀態:設計定稿、**尚未實作**(使用者要求先規劃)。
日期:2026-06-15

## 目標

任務完成時,在使用者**當下焦點所在的螢幕**浮出一個玻璃擬態 overlay,
**一句一句顯示**一段口語摘要、**同步語音朗讀**,並有隨語音律動的音波,
唸完自動淡出。讓 FreeCowork 完成任務後「像真的 AI 一樣開口回報」。

效果預覽:`freecowork-voice-demo.html`(獨立 demo,非專案檔)。

## 已確認的決策

| 項目 | 決定 |
|---|---|
| 觸發 | 任務完成(`on_done`),由設定 `announce_enabled` 開關 |
| 播報內容 | 請模型生 **1~2 句口語摘要**(失敗則 fallback) |
| TTS 引擎 | **系統內建 / Web Speech API**(overlay webview 直接用,Rust 不碰語音) |
| 位置 | 焦點螢幕的**下方中央** |
| 消失 | **唸完延遲 1~2 秒自動淡出** |
| 預設語音 | 偏好「自然女聲 zh-TW」(見下方「語音選擇」一節的務實策略) |

## 語音選擇(重要)

使用者在 demo 中偏好的音色是 **「Google 國語(臺灣)(zh-TW)」**。

**務實提醒**:該語音是 **Chrome 的 Web Speech 語音**(且 Google 語音為線上),
正式 app 跑在 **WebView2(Edge 引擎)**,可用語音清單不同 —— 通常是 Microsoft 語音
(如 `Microsoft HsiaoChen Online (Natural) - Chinese (Taiwan)`,或離線的 `HanHan` / `Zhiwei`)。
因此 demo 裡那個 Google 音色**在正式 app 不一定存在**。

**預設語音選用策略(挑最接近的可用語音)**:

1. 設定 `announce_voice` 有指定且清單中存在 → 用它。
2. 否則自動挑:名稱含 `Natural` 或 `Online`、且 `lang` 為 `zh-TW` 的語音(最接近 Google 那種自然音質)。
3. 再退回:任何 `lang` 開頭為 `zh` 的語音。
4. 再退回:系統預設語音(並記錄一次 log)。

設定視窗提供下拉,讓使用者從**實際可用清單**選擇並記住(存 `announce_voice`)。
若使用者堅持要 demo 那種 Google 音色 → 需改走雲端 TTS(本版範圍外,見「未來」)。

`utterance.lang = 'zh-TW'`、`rate ≈ 1.0`、`pitch ≈ 1.05`(可日後調)。

## 架構與元件

```
任務完成 on_done (ipc.rs)
  └─[announce_enabled] 生摘要(既有模型管線,短 prompt;失敗→fallback 句)
       └─ focused_monitor_rect() 取焦點螢幕 → 算下方中央座標
            └─ 顯示 announcer 視窗 + emit 摘要(announce payload)
                 └─ 前端:切句 → 逐句淡入 + Web Speech 朗讀 + 音波律動
                      └─ 唸完 +1~2s → 淡出 → 通知 Rust 隱藏視窗
```

1. **觸發(Rust,`ipc.rs` `on_done`)**:現有 toast 保留;新增「若開啟則播報」分支,
   把結果文字與結束碼交給摘要步驟。
2. **摘要(Rust,重用既有模型呼叫)**:短 prompt「用一句口語中文摘要這次任務結果」。
   失敗 / 逾時 → fallback(成功:「任務完成」;失敗:「任務結束,但有錯誤」)。
3. **Announcer 視窗(新 Tauri window,label `announcer`)**:
   `transparent` / `decorations:false` / `alwaysOnTop` / `skipTaskbar` /
   **`focus:false`(不搶使用者焦點)** / 自動淡出模式下點擊穿透。
   視覺沿用 demo(玻璃面板 + 字幕 + 音波 + 發光球)+ 既有 `fx.rs` acrylic。
4. **定位(重用)**:`focused_monitor_rect()`(已實作)取焦點螢幕,放下方中央
   (水平置中、底部上方留邊距)。偵測失敗 → 主螢幕。
5. **語音 + 動畫(前端 Web Speech)**:逐句 `SpeechSynthesisUtterance`,
   `onstart`/`onend` 驅動音波與逐句推進,`onboundary` 做逐字高亮(支援的瀏覽器)。
6. **設定**:`settings.json` 加 `announce_enabled: bool`(預設待定)與 `announce_voice: String`;
   設定視窗加開關 + 語音下拉。`announce_enabled` 預設**開啟**(可關)。

## 資料流 / 介面

- Rust → 前端事件 `announce`,payload:`{ text: String, code: i32 }`。
- 前端 → Rust 命令 `announcer_done`(唸完/淡出後請 Rust 隱藏 `announcer` 視窗)。

## 錯誤處理

- 摘要模型失敗/逾時 → fallback 句,照常播報。
- 無可用 TTS 語音 → 仍顯示字幕(無聲),不報錯。
- 焦點螢幕偵測失敗 → 主螢幕(同 palette 既有 fallback)。
- 連續多個任務完成 → 佇列或「打斷前一則」(預設:打斷前一則,只播最新)。

## 測試

- **純函式單元測試**:摘要文字切句(中文句末 。!? 與換行);fallback 句邏輯。
- **手動驗證**(同 palette):多螢幕下任務完成,overlay 出現在焦點螢幕下方中央、
  字幕逐句、語音朗讀、唸完淡出;設定開關有效;語音下拉切換生效。

## 範圍(YAGNI)

**第一版只做**:系統 TTS(Web Speech)+ 一句摘要 + 焦點螢幕下方中央自動淡出 + 開關 + 語音選擇。

**未來(本版不做)**:雲端 TTS(ElevenLabs/Azure,可得 demo 那種音色 + 真實音波)、
真實音訊波形分析、自訂語速/停留時間/位置、歷史播報重聽。

## 重用既有資產

`focused_monitor_rect()`(焦點螢幕)、palette 視窗的透明/置頂/acrylic 做法(`fx.rs`)、
既有模型呼叫管線、既有 `settings` 載入/儲存機制。
