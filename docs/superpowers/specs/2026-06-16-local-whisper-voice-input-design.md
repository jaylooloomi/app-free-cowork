# 設計規格:本地 Whisper 語音輸入(取代 Win+H)

狀態:設計定稿、**尚未實作**(使用者要求先規劃)。
日期:2026-06-16

## 目標

把目前「按麥克風鈕 → 合成 Win+H → 跳出 Windows 內建語音工具列」的做法,換成 **app 自己的本地語音轉文字**:
按鈕/快捷鍵錄音 → 本地 Whisper 辨識 → 文字直接填入面板輸入框。**不跳任何 Windows 工具列、完全離線、不送雲端**。

## 研究依據(2026-06,已查證)

- 主流聽寫 app(Typeless / Wispr Flow / Aqua / Talon…)**都不用 Win+H**:自己抓麥克風 → 自己跑 STT → 自己畫錄音 UI → 自己塞字。Win+H 只是微軟把這四步包成系統工具列。
- 兩大陣營:**雲端 STT + LLM 修稿**(Typeless 等,需連網)vs **本地 Whisper 家族**(Talon/Superwhisper/Handy,隱私/離線)。
- 本案選**本地 Whisper**(隱私、離線、不用 API key)。
- **Web Speech API 在 WebView2 不可用**(Edge 的 `SpeechRecognition` 是空殼,永不回結果)→ 必須走 Rust 端。

## 核心決策

| 項目 | 決定 |
|---|---|
| 引擎 | **本地 Whisper**,Rust `whisper-rs`(whisper.cpp FFI,靜態連結) |
| 範圍 | 只填入**面板自己的輸入框**(非系統級聽寫)→ 辨識完 emit 文字、前端 append,**不需注入別的 app** |
| 觸發 | 麥克風鈕 / `voice_hotkey`(預設 Alt+J)**切換**:按一下開始錄音,再按一下停止並辨識 |
| 模式 | **錄完才辨識**(非即時串流)—— 純 CPU 串流延遲高,先求準確簡單 |
| 模型 | 預設 `base`(~140MB);**首次使用時下載**到 app 資料夾,不灌進安裝包 |
| 語言 | 多語模型,中文可用;語言可設 auto |
| 錄音 UI | 沿用面板現有麥克風脈動動畫,辨識中顯示「辨識中…」 |

## 架構與資料流

```
按麥克風鈕 / Alt+J(開始)
  → IPC start_recording:cpal 開麥克風串流,把 f32 樣本收進緩衝(背景執行緒)
  → 面板進入「錄音中」狀態(脈動)
按麥克風鈕 / Alt+J(停止)
  → IPC stop_and_transcribe:
       停止 cpal → rubato 重採樣到 16kHz 單聲道 f32
       → whisper-rs 載入模型(首次:確認/下載)→ full() 辨識 → 取出文字
  → 回傳/emit 文字 → 前端把文字 append 進 input 欄位
```

- **音訊擷取**:`cpal`(預設輸入裝置),取得樣本格式後轉 f32;若非 16kHz/單聲道 → `rubato` 重採樣 + 混單聲道。
- **辨識**:`whisper-rs::WhisperContext` 載入 ggml 模型,`full()` 跑辨識,串接 segment 文字。
- **回傳**:辨識在 spawn_blocking/執行緒做(避免卡 UI);完成後 emit `voice-transcript` 事件給面板,前端 append。

## 模型管理

- 路徑:app 資料夾(`config_dir/free-claude-code/models/ggml-base.bin`)。
- 首次使用:若模型不存在 → 從 Hugging Face ggml 釋出網址下載(顯示進度);`whisper-rs` 模型缺時不會自動拉,需自己處理。
- 下載用既有 `http` 模組或 `ureq`(已相依)。

## 前置條件 / 建置(這個比前面功能重)

- **CMake**(VS「Desktop development with C++」工作負載已含)。
- **LLVM / libclang**:`whisper-rs` 用 bindgen,需設 `LIBCLANG_PATH` 指向 LLVM bin;**可能要另裝 LLVM**。
- MSVC 工具鏈(Tauri 本來就需要,已裝)。
- whisper.cpp 是大型 C++ 編譯,**首次編譯較久、吃記憶體**(本機曾 OOM)→ 建置時限制平行度、留足記憶體。
- 預設 **CPU** 後端(相容性);`cuda`/`vulkan` 為日後 opt-in。

## 設定(最小)

- `voice_model`:`"base"`(預設)/ `"small"`。
- `voice_lang`:`"auto"`(預設)/ `"zh"` / `"en"`。
- (沿用既有 `voice_hotkey`,不需新增熱鍵。)

## 錯誤處理

- 無麥克風 / 無權限 → 通知,該次取消。
- 模型未下載 → 觸發下載並提示「首次下載中…」;下載失敗 → 通知。
- 辨識失敗 / 無語音 → 安靜結束,輸入框不變。
- 辨識耗時 → 錄音停止後顯示「辨識中…」,完成才填字。

## 測試

- **純函式單元測試**:重採樣參數計算、模型檔路徑、語言參數對應、文字後處理(去除 whisper 的 `[BLANK_AUDIO]` 等標記)。
- **手動驗證**:錄一句中文/英文 → 確認填入輸入框;無語音 → 不填;切換錄音狀態 UI 正確;首次下載模型流程。

## 範圍(YAGNI)

**v1**:錄完才辨識、填入面板輸入框、base 模型(首次下載)、CPU、沿用麥克風鈕/Alt+J、移除 Win+H。

**未來(不做)**:即時串流逐字、GPU 加速、`small`/`large` 模型切換 UI、**LLM 修稿**(去贅字/格式化 —— 本 app 已有模型可重用)、系統級(任何 app)聽寫、自訂錄音音波。

## 取代既有

移除 `voice.rs` 的 Win+H 合成(`trigger_voice_typing` / `build_win_h_inputs` / `start_voice_input`),改為新的錄音/辨識流程;`onMic`(前端)改呼叫新 IPC。
