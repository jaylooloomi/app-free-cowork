# 本地 Whisper 語音輸入 實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 或 superpowers:executing-plans 逐任務實作。步驟用 `- [ ]` 追蹤。
> 對應規格:`docs/superpowers/specs/2026-06-16-local-whisper-voice-input-design.md`

**Goal:** 用本地 Whisper(`whisper-rs`)取代 Win+H:錄音 → 離線辨識 → 文字填入面板輸入框,自己的錄音 UI、不跳 Windows 工具列。

**Architecture:** `cpal` 抓麥克風 → `rubato` 重採樣 16kHz 單聲道 → `whisper-rs` 辨識 → emit 文字回前端 append 進 input。錄完才辨識(非串流)。模型首次下載到 app 資料夾。

**Tech Stack:** Rust / Tauri 2、`whisper-rs`(whisper.cpp)、`cpal`、`rubato`、`ureq`(已相依,下載模型)、SvelteKit。

---

## ⚠️ 前置條件(動工前先確認 / 安裝)

- **CMake** — VS「Desktop development with C++」工作負載已含(本機已裝)。
- **LLVM / libclang** — `whisper-rs` 用 bindgen,需 `LIBCLANG_PATH` 指向 LLVM 的 `bin`。**本機可能尚未安裝**:用 `winget install LLVM.LLVM`(或 VS 的「C++ Clang tools」元件),裝完設 `LIBCLANG_PATH`(例 `C:\Program Files\LLVM\bin`)。
- whisper.cpp 首次編譯**較久、吃記憶體**(本機曾 OOM)→ 用 `CARGO_BUILD_JOBS` 限制平行度、確認可用記憶體充足。
- 預設 **CPU** 後端(不開 cuda/vulkan,求相容)。

## 檔案結構

- **修改** `launcher/src-tauri/Cargo.toml` — 加 `whisper-rs`、`cpal`、`rubato`。
- **建立** `launcher/src-tauri/src/stt.rs` — 純邏輯(模型路徑、文字後處理、重採樣參數)+ whisper 辨識封裝。
- **修改** `launcher/src-tauri/src/ipc.rs` — `AppState` 加錄音狀態;`start_recording`/`stop_and_transcribe` 命令;emit `voice-transcript`。
- **修改** `launcher/src-tauri/src/voice.rs` — **移除** Win+H 合成(或整檔刪除,改放 cpal 擷取)。
- **修改** `launcher/src-tauri/src/lib.rs` — 註冊命令;模型下載輔助;移除舊 `start_voice_input` 註冊。
- **修改** `launcher/src-tauri/src/settings.rs` — `voice_model` / `voice_lang`。
- **修改** 前端 `src/lib/api.ts`、`src/lib/Palette.svelte`(`onMic` 改錄音切換 + transcript 監聽)、`strings.ts`(辨識中/下載中字串)。

---

## Task 1:相依與建置環境

- [ ] **Step 1:** 確認 LLVM 已裝(`where clang` 或 `Test-Path "C:\Program Files\LLVM\bin\libclang.dll"`);未裝則 `winget install LLVM.LLVM`,並設 `LIBCLANG_PATH`。
- [ ] **Step 2:** `Cargo.toml` `[dependencies]` 加:
```toml
whisper-rs = "0.14"
cpal = "0.15"
rubato = "0.16"
```
- [ ] **Step 3:** 建立空的 `stt.rs`(`//! 本地 Whisper STT`)+ `lib.rs` 加 `pub mod stt;`,跑 `cargo build --release`(限制 jobs)確認 whisper-rs 能編譯/連結(這步驗證前置環境)。
Run: `CARGO_BUILD_JOBS=4 cargo build --release --manifest-path launcher/src-tauri/Cargo.toml` — Expected: 編過(whisper.cpp 編譯較久)。
- [ ] **Step 4: Commit** `chore(stt): add whisper-rs/cpal/rubato deps + stt module`

## Task 2:文字後處理(純函式,TDD)

**Files:** `stt.rs`

- [ ] **Step 1: 失敗測試**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleans_whisper_artifacts() {
        assert_eq!(clean_transcript("  [BLANK_AUDIO] 你好 (打字聲) "), "你好");
        assert_eq!(clean_transcript("[_BEG_] hello [_TT_50]"), "hello");
        assert_eq!(clean_transcript("   "), "");
    }
}
```
- [ ] **Step 2:** `cargo test --release --lib stt::` → 失敗(`clean_transcript` 未定義)。
- [ ] **Step 3: 實作**
```rust
/// 清掉 whisper 常見的非語音標記與多餘空白:[BLANK_AUDIO]、(背景聲)、[_xxx_] 等。
pub fn clean_transcript(raw: &str) -> String {
    let mut s = raw.to_string();
    // 去掉 [..] 與 (..) 標記
    for (open, close) in [('[', ']'), ('(', ')'), ('（', '）')] {
        while let (Some(i), Some(j)) = (s.find(open), s.find(close)) {
            if i < j { s.replace_range(i..=j, ""); } else { break; }
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string()
}
```
- [ ] **Step 4:** `cargo test --release --lib stt::` → PASS。
- [ ] **Step 5: Commit** `feat(stt): clean_transcript pure fn + tests`

## Task 3:模型路徑 + 下載

**Files:** `stt.rs`、`lib.rs`

- [ ] **Step 1:** `stt.rs` 加 `pub fn model_path(name: &str) -> PathBuf`(`config_dir/free-claude-code/models/ggml-<name>.bin`);測試路徑組合正確。
- [ ] **Step 2:** `pub fn ensure_model(name: &str) -> Result<PathBuf, String>`:存在則回路徑;不存在則從 `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-<name>.bin` 下載到該路徑(用既有 `http`/`ureq`,寫 `.part` 再 rename)。下載期間 emit 進度(可選)。
- [ ] **Step 3:** `cargo build` 驗證。Commit `feat(stt): model path + first-run download`

## Task 4:錄音(cpal)+ 重採樣(rubato)

**Files:** `stt.rs`、`ipc.rs`(AppState 錄音緩衝)

- [ ] **Step 1:** AppState 加 `recording: Mutex<Option<RecordingState>>`(含 cpal stream handle、樣本緩衝 `Arc<Mutex<Vec<f32>>>`、來源 sample_rate/channels)。
- [ ] **Step 2:** `stt::start_capture() -> RecordingState`:開預設輸入裝置,串流 callback 把樣本(轉 f32、若多聲道取平均)推進緩衝。
- [ ] **Step 3:** `stt::resample_to_16k_mono(samples, src_rate) -> Vec<f32>`(rubato);純函式部分(計算 ratio)可單測。
- [ ] **Step 4:** `cargo build` 驗證。Commit `feat(stt): cpal capture + 16k mono resample`

## Task 5:Whisper 辨識封裝

**Files:** `stt.rs`

- [ ] **Step 1:** `pub fn transcribe(samples_16k: &[f32], model: &Path, lang: &str) -> Result<String, String>`:
```rust
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};
let ctx = WhisperContext::new_with_params(model.to_str().unwrap(), WhisperContextParameters::default())
    .map_err(|e| e.to_string())?;
let mut state = ctx.create_state().map_err(|e| e.to_string())?;
let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
if lang != "auto" { p.set_language(Some(lang)); }
state.full(p, samples_16k).map_err(|e| e.to_string())?;
let n = state.full_n_segments().map_err(|e| e.to_string())?;
let mut out = String::new();
for i in 0..n { out.push_str(&state.full_get_segment_text(i).unwrap_or_default()); }
Ok(clean_transcript(&out))
```
(版本 API 名稱以實際 `whisper-rs` 0.14 為準,編譯時對齊。)
- [ ] **Step 2:** `cargo build` 驗證。Commit `feat(stt): whisper transcribe wrapper`

## Task 6:IPC 命令(start / stop+transcribe)

**Files:** `ipc.rs`、`lib.rs`(註冊)

- [ ] **Step 1:** `#[tauri::command] start_recording(state)`:`stt::start_capture()` 存進 AppState.recording;若已在錄則忽略。
- [ ] **Step 2:** `#[tauri::command] stop_and_transcribe(app, state) -> Result<String,String>`:取出 recording、停 stream、拿緩衝樣本 → `spawn_blocking`(resample → ensure_model → transcribe)→ 回傳文字。錯誤回 Err。
- [ ] **Step 3:** `lib.rs` `generate_handler!` 加 `ipc::start_recording, ipc::stop_and_transcribe`;移除 `ipc::start_voice_input`。
- [ ] **Step 4:** `cargo build`。Commit `feat(stt): start_recording + stop_and_transcribe IPC`

## Task 7:前端(錄音切換 + 填字 + UI)

**Files:** `api.ts`、`Palette.svelte`、`strings.ts`

- [ ] **Step 1:** `api.ts` 加 `startRecording()`、`stopAndTranscribe()`;移除 `startVoiceInput`。
- [ ] **Step 2:** `Palette.svelte` `onMic()` 改:未錄音 → `startRecording()` + `listening=true`;錄音中 → `stopAndTranscribe()` →(顯示「辨識中…」)→ 得到文字 append 進 `input`、`listening=false`。
- [ ] **Step 3:** 沿用現有 `.mic.listening` 脈動;辨識中用 `transient`/狀態列顯示 `S.voiceTranscribing`。
- [ ] **Step 4:** `strings.ts` 加 `voiceTranscribing`(「辨識中…」/「Transcribing…」)、`voiceModelDownloading`(「首次下載語音模型…」/「Downloading voice model…」)(zh+en)。
- [ ] **Step 5:** `npm --prefix launcher run check` → 0 errors。Commit `feat(stt): palette voice record/transcribe UI`

## Task 8:移除 Win+H

**Files:** `voice.rs`、`lib.rs`、`ipc.rs`

- [ ] **Step 1:** 刪 `voice.rs` 的 Win+H 合成(`build_win_h_inputs`/`trigger_voice_typing`/相關測試),或整檔改放 cpal 擷取(Task 4 若已搬入則此處清乾淨)。
- [ ] **Step 2:** 移除 `start_voice_input` 命令與註冊、`win_h` 相關。
- [ ] **Step 3:** `cargo test --release --lib`(確認無殘留參照)。Commit `refactor(voice): remove Win+H voice typing (replaced by local whisper)`

## Task 9:設定(voice_model / voice_lang)

**Files:** `settings.rs`、`ipc.rs`(overlay + 測試)、`api.ts`、`Settings.svelte`、`strings.ts`

- [ ] **Step 1:** `settings.rs` 加 `voice_model: String`(預設 "base")、`voice_lang: String`(預設 "auto")+ Default + 測試斷言。
- [ ] **Step 2:** `ipc.rs` `overlay_ui_fields` 加兩欄 + 更新測試 incoming 字面值 + 斷言。
- [ ] **Step 3:** `api.ts` Settings 介面加兩欄;`Settings.svelte` 加「語音模型」下拉(base/small)+「辨識語言」下拉(auto/中文/English);save 帶上。`strings.ts` 加標籤(zh+en)。
- [ ] **Step 4:** `stop_and_transcribe` 改讀 settings 的 model/lang。
- [ ] **Step 5:** check + build。Commit `feat(stt): voice model/language settings`

## Task 10:建置、安裝、驗證

- [ ] **Step 1:** `CARGO_BUILD_JOBS=4 npm --prefix launcher run tauri -- build --no-bundle`(留意首次 whisper.cpp 編譯時間/記憶體)→ 覆蓋安裝 → explorer 重啟。
- [ ] **Step 2:** 手動驗證:Alt+H 開面板 → 按麥克風(或 Alt+J)→ 說一句中文 → 再按停止 →(首次:模型下載)→「辨識中…」→ 文字填入輸入框;無語音不填;設定切模型/語言生效;**確認不再跳 Windows 工具列**。
- [ ] **Step 3:** 收尾(推分支 / 合併)。

---

## 自我檢查(對照規格)
引擎=本地 whisper-rs ✓(T1/T5)、填面板輸入框 ✓(T7)、錄音切換+UI ✓(T7)、模型首次下載 ✓(T3)、移除 Win+H ✓(T8)、設定 ✓(T9)、前置(LLVM/CMake/記憶體)✓(前置區+T1)。
實作時需對齊:`whisper-rs` 0.14 實際 API 名稱;cpal 樣本格式分支(i16/f32);rubato 版本 API。
