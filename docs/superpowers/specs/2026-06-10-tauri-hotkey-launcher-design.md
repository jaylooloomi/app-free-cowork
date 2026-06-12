# Free Claude Code — 全域快捷鍵 AI 啟動器 設計文件

- 日期:2026-06-10
- 狀態:已核准(使用者授權自主開發至可交付)
- 分支:`feature/tauri-launcher`

## 1. 目標

讓任何一台全新的 Windows 電腦,在裝完一個 ~10MB 的小程式之後,隨時按下快捷鍵(預設 `Alt+H`)喚出輸入框,輸入自然語言需求(例:「幫我整理桌面,並且建立資料夾分類」),按 Enter 後系統自動完成:環境偵測 → 缺件自動安裝(Ollama、Claude Code)→ 啟動 Claude Code 接上 Ollama 免費雲端模型 → 帶入需求自動執行。

使用者全程只做兩件事:**按快捷鍵、打一句話**。首次使用額外多一件:點一次 ollama.com 瀏覽器登入。

### 既定決策(與使用者逐項確認)

| 決策點 | 結論 |
|---|---|
| 發佈對象 | 公開發佈(不特定大眾) |
| 技術路線 | Tauri v2 系統匣常駐 app(方案三) |
| 快捷鍵 | 預設 `Alt+H`,可在設定改(註:與 Office 功能區衝突,設定頁註記) |
| 執行呈現 | 預設開終端機視窗即時看 Claude 工作;設定可切背景模式 |
| 預設模型 | ~~`minimax-m2.7:cloud`~~ → `minimax-m2.5:cloud`(2026-06-12 修訂:實測 m2.7 為訂閱模型,403),設定可改,自動 fallback |
| 權限模式 | 預設 `--dangerously-skip-permissions`(零打擾);設定提供「謹慎模式」開關(切回 `--permission-mode acceptEdits`) |
| 遙測 | 零遙測,當賣點 |

## 2. 已驗證的技術事實(2026-06-10,逐字對照官方文件與原始碼)

本設計的所有關鍵假設皆經過查證,實作時**不可**憑印象改寫:

1. **啟動指令**(docs.ollama.com/integrations/claude-code 官方範例格式):
   `ollama launch claude --model <model> --yes -- <claude 參數與 prompt>`
   - `--model` 跳過互動選單;headless 環境必須同時帶 `--yes`
   - `--` 之後的參數逐字傳給 claude binary;positional prompt = 互動模式自動送出第一句、session 保持開啟;`-p` = 非互動跑完即退
   - `ollama launch` 需 Ollama ≥ 0.15.0;**≥ 0.15.6 會自動下載缺少的模型**(本設計要求 ≥ 0.15.6)
   - `ollama launch` **不會**安裝 claude binary(找不到會報錯),bootstrap 必須先裝
   - 底層機制:設定 `ANTHROPIC_BASE_URL=http://127.0.0.1:11434`、`ANTHROPIC_AUTH_TOKEN=ollama` 等環境變數後 spawn claude(Ollama ≥ 0.14 提供 Anthropic 相容 `/v1/messages` 端點)
2. **Claude Code 原生安裝器**(code.claude.com/docs/en/setup):
   `irm https://claude.ai/install.ps1 | iex` — 免 Node.js、免管理員、Anthropic 簽章、自動更新,裝到 `%USERPROFILE%\.local\bin\claude.exe`。**舊版 npm 安裝路線廢棄。**
3. **雲端模型清單 JSON API**(docs.ollama.com/cloud 官方記載):
   `GET https://ollama.com/api/tags` — 免認證,回傳全部雲端模型(名稱不含 `:cloud` 後綴;經本機 daemon 使用時要加)。**舊版 Playwright 爬蟲路線廢棄。**
4. **`ollama pull X:cloud` 只下載幾百 byte 的 manifest stub**,秒完成,不下載權重。
5. **雲端模型需要 ollama.com 帳號**:`ollama signin` 開瀏覽器授權,一次性,無 token 參數可繞過。免費額度為 GPU 時間制(5 小時滾動 session + 每週上限,同時 1 個模型),**非**請求次數制 — 限流時換模型無效,只能等重置。
6. **Ollama Windows 安裝免管理員**:winget 套件為 per-user Inno Setup;直接下載 `https://ollama.com/download/OllamaSetup.exe` 後 `/VERYSILENT /SP- /SUPPRESSMSGBOXES` 靜默安裝(Inno 慣例;`/S` 是 NSIS 慣例,不可用)。
7. **winget 在全新機器不保證可用**(首次登入後才非同步註冊),所有 winget 步驟必須有直接下載 fallback。
8. **PowerShell 下載的檔案沒有 Mark-of-the-Web**,不觸發 SmartScreen;瀏覽器下載的未簽章 exe 會觸發。`irm | iex` 在預設 Restricted 執行原則下可用(它是命令不是腳本檔)。
9. **`minimax-m2.7:cloud` 存在**且為現役 agentic 基準最強的雲端模型之一(官方 Claude Code 整合頁推薦清單成員)。

## 3. 架構總覽

```
使用者 ──Alt+H──▶ ┌────────────────────────────────────────┐
                  │  常駐啟動器(Tauri v2 系統匣 app)       │
                  │  輸入面板 ─▶ 環境醫生 ─▶ 啟動器          │
                  │                │                        │
                  │                ▼(缺件時)               │
                  │            安裝精靈                      │
                  └────────────────────┬───────────────────┘
                                       ▼
                              Claude Code(claude.exe)
                                       │ /v1/messages
                                       ▼
                              Ollama 本機服務 :11434
                                       │
                                       ▼
                              ollama.com 雲端(minimax-m2.7 推論)
```

- Rust 核心:快捷鍵、環境醫生、安裝引擎、程序啟動、設定、log
- WebView UI(Svelte + TypeScript + Vite):輸入面板、安裝精靈、設定頁
- Tauri 官方外掛:global-shortcut、autostart、single-instance、updater、notification、store

## 4. 元件規格

### 4.1 全域快捷鍵 + 輸入面板

- global-shortcut 外掛註冊設定中的組合鍵(預設 `Alt+H`)。註冊失敗(被占用)→ 系統通知 + 開設定頁引導改鍵。
- 面板視窗:無邊框、置頂、不進工作列、螢幕水平置中 / 垂直約 25% 處,寬 ~640px。**show/hide 重用同一視窗**(冷建立太慢),app 啟動時預建隱藏。
- 行為:喚出即聚焦輸入框;`Enter` 送出;`Esc` 或失焦即隱藏;`↑`/`↓` 翻閱歷史(存最近 20 筆於設定檔)。
- 面板下緣狀態列:顯示目前模型與環境狀態(正常時只顯示模型名;異常時顯示「離線」「需要登入」「首次使用將自動安裝元件」等)。

### 4.2 環境醫生(doctor)

狀態機,輸出 `Ready | NeedsSetup(缺件清單) | Offline | NeedsSignin | Degraded(說明)`。

| 檢查 | 方法 | 快/慢 |
|---|---|---|
| claude 存在 | `%USERPROFILE%\.local\bin\claude.exe` 存在或 PATH 找得到 | 快 |
| ollama 存在且 ≥ 0.15.6 | `ollama --version` 解析 semver | 快 |
| Ollama 服務活著 | `GET http://127.0.0.1:11434/api/version`(timeout 1s);死的就靜默 spawn `ollama serve`(隱藏視窗)等待最多 10s | 快 |
| 模型在雲端目錄 | `GET https://ollama.com/api/tags`(快取 24h) | 慢(背景刷新) |
| 模型已註冊本機 | `ollama list` 包含;沒有就 `ollama pull <m>:cloud`(秒完成) | 快 |
| 已登入 ollama.com | **無靜默探測法**(`ollama signin` 未登入時會開瀏覽器,不可當 probe)。策略:狀態三值 `Unknown/Yes/No`;首次成功執行後記 `Yes`;執行時收到 auth 錯誤 → 記 `No` 並導回精靈登入步驟 | — |

- 快取:完整體檢於 app 啟動與精靈完成後執行;日常送出只做快路徑(檔案存在 + port ping),毫秒級。
- 所有外部指令呼叫走 trait 注入,單元測試 mock 輸出。

### 4.3 安裝精靈(wizard)

醫生回報 `NeedsSetup` 時出現的視窗,步驟即時打勾、冪等、已裝自動跳過:

1. **安裝 Ollama** — `winget install -e --id Ollama.Ollama --scope user`;winget 不存在或失敗 → 下載 `OllamaSetup.exe` 跑 `/VERYSILENT /SP- /SUPPRESSMSGBOXES`;完成後重讀 PATH(註冊表 Machine+User)+ 明確補 `%LOCALAPPDATA%\Programs\Ollama`
2. **安裝 Claude Code** — 執行官方 `irm https://claude.ai/install.ps1 | iex`;完成後補 `%USERPROFILE%\.local\bin` 到 session PATH
3. **登入 ollama.com** — 按鈕觸發 `ollama signin`(開瀏覽器);UI 輪詢偵測(`signin` 子程序結束碼/輸出);提供「重試」
4. **註冊模型** — `ollama pull minimax-m2.7:cloud`(stub,秒完成)
5. **聲明頁** — 「本工具會讓 AI 自動執行檔案操作(預設不逐項確認)」+「開始使用」按鈕

- 觸發精靈前,把使用者剛輸入的需求**暫存**;精靈完成後**自動繼續執行**,不需重打。
- 升級情境:ollama 版本過舊 → 精靈只跑步驟 1(同路徑升級)。

### 4.4 啟動器(launcher)

- 指令組裝(arg 陣列 spawn,不經 shell 字串拼接,杜絕引號注入):
  - 前景:`ollama launch claude --model <m> --yes -- --dangerously-skip-permissions "<需求>"`
  - 謹慎模式:`--dangerously-skip-permissions` 換成 `--permission-mode acceptEdits`
  - 背景:`-- -p --dangerously-skip-permissions "<需求>"`,隱藏視窗,stdout/stderr 導入 log 檔,結束後 Windows 通知(成功/失敗 + 點擊開 log)
- 前景終端機:偵測 `wt.exe`(Windows Terminal)優先,fallback `conhost`(`cmd /c start`)。
- 工作目錄:設定值,預設 `%USERPROFILE%`。
- 隱藏 CLI 介面(兼測試掛鉤與進階用法):`free-claude-code.exe --run "<需求>"` 走與面板送出完全相同的路徑;`--show-palette` 喚出面板。

### 4.5 設定與常駐

- 設定檔:`%APPDATA%\free-claude-code\settings.json`(tauri-plugin-store)。欄位:hotkey、model、cautious_mode、background_mode、working_dir、autostart、history[]、signin_state、catalog_cache。
- 設定頁可改全部欄位;模型下拉清單來自 `ollama.com/api/tags`(僅列雲端目錄)。
- 系統匣選單:開啟輸入面板 / 設定 / 結束。single-instance:重複啟動 = 喚出面板。
- autostart 外掛(HKCU Run)。Tauri updater 對 GitHub Releases 檢查更新(啟動時 + 每日),有更新通知使用者、退出時安裝。Ollama 與 Claude Code 各自有自動更新,醫生只把守最低版本。

## 5. 錯誤處理

| 情境 | 行為 |
|---|---|
| 免費額度用完(429/quota) | 通知「免費額度已用完,稍後重置」;不自動重試(限制綁帳號,換模型無效) |
| 斷網 | 面板狀態列顯示「離線 — 雲端模型需要網路」,送出鈕停用 |
| 登入失效 | 執行時 auth 錯誤 → `signin_state=No` → 自動帶回精靈步驟 3 |
| 模型下架 | 醫生對照 catalog → fallback 順序:設定值 → `minimax-m2.7:cloud` → `qwen3-coder-next:cloud` → 目錄第一個;切換時通知 |
| 快捷鍵被占用 | 通知 + 開設定頁 |
| 精靈步驟失敗 | 該步驟顯示錯誤 + 「重試」;log 全程落盤 |
| log | `%APPDATA%\free-claude-code\logs\`,每次執行一檔,保留最近 30 個 |

## 6. 安全與權限

- 預設 `--dangerously-skip-permissions` 為使用者明確決策;精靈最後一頁有知情聲明;設定提供謹慎模式。
- 所有子程序以 arg 陣列 spawn,使用者輸入永不進入 shell 字串。
- 不收集任何遙測;設定與 log 全在本機。
- 不需要管理員權限(全鏈 per-user)。

## 7. 發佈與更新

- 通路:GitHub Releases(NSIS 安裝包 + updater manifest;含 WebView2 bootstrapper)→ winget manifest(主力,免 SmartScreen)→ `irm <repo>/install.ps1 | iex`(改為下載安裝主程式)。
- 簽章(開放事項):Azure Artifact Signing 個人限美/加;台灣個人可走 Certum 開源憑證(~€69/年)或 SignPath(OSS 免費),v1 可先以 winget 通路無簽章上線。

## 8. 測試策略

1. **單元測試(Rust)**:semver 比較、指令組裝(含特殊字元需求字串)、fallback 選擇、醫生狀態機(mock 指令輸出)、設定序列化。
2. **建置驗證**:`cargo test` + `cargo clippy` + `npm run check` + `tauri build` 完整出包。
3. **本機 E2E(開發機)**:啟動 app → `--show-palette`/`--run` 走完真實路徑 → 對沙盒資料夾(非真桌面)執行一個整理任務 → 驗證終端機開啟、Claude 執行、log 落盤。
4. **全新電腦 E2E(Windows Sandbox)**:`.wsb` 掛載安裝包,沙盒內跑安裝 → 首次精靈 → 驗證 Ollama/Claude Code 裝起來、流程走到 signin 步驟(signin 需真帳號,沙盒內以人工或跳過驗證)。
5. **人工驗收清單(交付時附)**:真帳號 signin、真桌面整理任務、快捷鍵改鍵、背景模式。

## 9. v1 範圍與非目標

- ✅ v1:上述全部元件與流程,介面繁體中文(字串集中一檔,預留 i18n)。
- ❌ 非目標:多語系、遙測、模型自動評測、macOS/Linux、雙擊裸 Alt(改用組合鍵後不需 hook;若未來要做,升級路徑為低階鍵盤 hook sidecar)。

## 10. 開放事項

1. 程式碼簽章供應商與時程(§7)。
2. 產品名:暫用 repo 名 `free-claude-code`(顯示名 Free Claude Code);使用者可後續更名。
3. `ollama signin` 完成偵測的具體機制(子程序結束碼 vs 輸出解析)— 實作時以真實行為為準。

## 11. 開發流程

- 分支 `feature/tauri-launcher`;`main` 不動,新版驗收後才談合併。
- 每完成一個可驗證的小任務即 commit(測試過了才 commit),commit 即可回復點。
- 不主動 push;合併與發佈由使用者決定。
- 現有 `setup.ps1`/`pull_ollama_cloud_model.py`/`uninstall.ps1` 保留於 main 不動;本分支以 Tauri 專案結構重組 repo。
