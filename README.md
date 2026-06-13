# Free Claude Code

**按 Alt+H,打一句話,AI 幫你完成 — 免費、免 API key**

---

## 這是什麼

Free Claude Code 是一個 Windows 系統匣常駐啟動器(Tauri v2)。按下 `Alt+H` 喚出輸入框,輸入自然語言需求後按 Enter,應用程式會自動安裝並接上 Ollama 雲端免費模型,再由 Claude Code 自動執行你的需求。

**首次使用唯一需要手動完成的步驟:**前往 [ollama.com](https://ollama.com) 以瀏覽器免費註冊/登入帳號。之後一切自動進行。

> **模型說明:**本工具透過 Ollama 雲端平台使用開源模型(預設 minimax-m2.5;部分新模型如 minimax-m2.7 需付費訂閱),並非 Anthropic 官方 Claude 商業模型。Ollama 免費額度受 GPU 時間限制,使用量較大時可能需要等待配額重置。

---

## 安裝(三選一)

### 1. 直接下載安裝檔(最簡單)

至 [Releases 頁面](https://github.com/jaylooloomi/free-claude-code/releases) 下載最新的 `*-setup.exe`,雙擊執行即可(免系統管理員)。

### 2. 一行指令安裝

在 PowerShell 貼上以下指令:

```powershell
irm https://raw.githubusercontent.com/jaylooloomi/free-claude-code/main/install.ps1 | iex
```

指令會自動抓取最新版本並靜默安裝。

### 3. winget(規劃中)

```powershell
winget install jaylooloomi.FreeClaudeCode
```

> 上架審核中,敬請期待。

---

## 使用方式

1. 安裝後應用程式自動常駐系統匣。
2. 按 `Alt+H` 喚出輸入框。
3. 輸入需求,例如:
   - `幫我整理桌面,並且建立資料夾分類`
   - `把 Downloads 裡的 PDF 重新命名為日期開頭`
4. 按 Enter 送出。

**首次使用**會執行安裝精靈(全程免系統管理員),依序安裝 Ollama、Claude Code,並引導至 ollama.com 瀏覽器登入。完成後自動執行你輸入的需求。

---

## 設定

按系統匣圖示 → **設定**,或在輸入框按 `Esc` 後從系統匣開啟設定視窗。

| 選項 | 說明 |
|------|------|
| 快捷鍵 | 預設 `Alt+H`,可改為任意組合(如 `Ctrl+Alt+Space`) |
| 模型 | 從 Ollama 雲端目錄選擇可用模型 |
| 謹慎模式 | 開啟後 AI 執行危險操作前會逐項確認,取代完全自動模式 |
| 背景模式 | 不開新終端機視窗,任務完成後以系統通知告知結果 |
| 工作目錄 | AI 執行任務的預設目錄(留空 = 使用者家目錄) |
| 開機自啟 | 登入 Windows 後自動常駐(預設開啟) |

---

## 隱私與安全

- **零遙測:**應用程式不收集任何使用資料,不傳送遙測。
- **預設完全自動:**預設使用 `--dangerously-skip-permissions`,AI 執行檔案操作時不逐項確認。如有疑慮請在設定中開啟**謹慎模式**。
- **安全的指令呼叫:**所有子程序以 arg 陣列方式 spawn,不經過 shell 字串插值,避免指令注入風險。

---

## 系統需求

- Windows 10 22H2 以上 或 Windows 11
- 網路連線(首次安裝及雲端模型呼叫皆需要)

---

## 開發

### 專案結構

```
launcher/           ← Tauri v2 應用程式
├── src/            ← 前端 (Svelte + TypeScript)
│   └── lib/        ← UI 元件與 API 包裝
└── src-tauri/      ← Rust 後端
    └── src/        ← 各功能模組
```

### 開發指令

```powershell
# 安裝前端依賴
cd launcher && npm install

# 開發模式(熱重載)
npm run tauri dev

# 執行 Rust 單元測試
cd src-tauri && cargo test

# 產生正式安裝包
npm run tauri build
# 產出: src-tauri/target/release/bundle/nsis/*-setup.exe
```

---

## 已知限制

- **免費額度限制:**Ollama 雲端免費方案有 GPU 時間配額,大量使用後需等待配額重置(限制綁帳號,更換模型無效)。
- **模型品質:**使用的是 Ollama 雲端開源模型,非 Anthropic 官方 Claude 商業模型,能力與穩定性有所不同。
- **僅支援 Windows:**目前僅支援 Windows 10 22H2+ / Windows 11,macOS/Linux 版本尚未規劃。
- **v1 尚無自動更新:**新版本請重新下載安裝(規劃中)。
- 每個工作目錄**第一次**啟動時,Claude Code 會顯示自己的資料夾信任確認(按 Enter 一次即可,之後同目錄不再出現;這是 Claude Code 的官方安全機制,本工具不繞過)
- **背景模式完成通知不可點擊開啟記錄:**請從設定 → 開啟記錄資料夾查看。

---

## English Summary

**Free Claude Code** is a Windows system-tray launcher that lets you run AI-powered tasks for free — no API key required. Press `Alt+H`, type what you need, and the app automatically installs Ollama + Claude Code, connects to free cloud models, and executes your request.

**One-line install:**
```powershell
irm https://raw.githubusercontent.com/jaylooloomi/free-claude-code/main/install.ps1 | iex
```

Or download the setup from [Releases](https://github.com/jaylooloomi/free-claude-code/releases).

> Uses open-source models via Ollama cloud (not Anthropic's commercial Claude). Free tier subject to GPU quota limits.

---

## 授權 / License

MIT
