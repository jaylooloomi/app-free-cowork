<div align="center">

# FreeCowork

### 你的 AI 同事,一個快捷鍵就到。

**按 `Alt+H` · 說出你要什麼 · 它幫你做完 —— 免費、免 API key、零設定。**

[![Release](https://img.shields.io/github/v/release/jaylooloomi/FreeCowork?label=download&style=flat-square)](https://github.com/jaylooloomi/FreeCowork/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?style=flat-square)](#系統需求)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%20v2-24C8DB?style=flat-square)](https://tauri.app)
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](#授權)

🌐 &nbsp; [English](README.md) &nbsp;·&nbsp; **繁體中文**

</div>

---

> FreeCowork 是一個 Windows 系統匣常駐小工具。按 `Alt+H` 叫出輸入框,用講的或打字說出需求,AI 就**直接在你電腦上動手做**——整理檔案、改名、查資料、開程式都行。可用**免費**的 Ollama 雲端開源模型(只需到 ollama.com 免費登入一次,免 API key),或接你自己的 Anthropic Claude 帳號。設計理念:**越無腦越好**,讓完全不懂技術的人也能享受 AI 幫忙幹活。

---

## 痛點

AI 現在真的能幹活了 —— 但對大多數人來說,這份能力被一道牆擋在外面:

- **真正強的工具,門檻都在技術設定。** 命令列 AI agent 和 API 意味著終端機、API key、帳單後台、設定檔。多數人連第一個指令都還沒下就放棄了。
- **聊天助手只會「說」,不會「做」。** ChatGPT 或 Claude.ai 會*告訴你*怎麼把 200 個檔案改名 —— 但你還是得自己動手。它住在瀏覽器分頁裡,不在你的桌面上。
- **成本與金鑰是硬門檻。** 「輸入信用卡 / 貼上 API key」直接擋掉非技術使用者。
- **不斷切換情境。** 離開手邊工作 → 開另一個工具 → 描述問題 → 把答案複製回來 → 自己再做一次。

結果就是:最需要 AI 助手的人,往往是最沒辦法把它設定起來的人。

## 解法

**FreeCowork 把這一切收進一個快捷鍵。**

```
Alt+H  →  「幫我把桌面依檔案類型整理成資料夾」  →  Enter
                         ↓
        AI 真的在你的電腦上把它做完,而且即時顯示過程。
```

不用終端機、不用 API key、不用讀文件。App 首次執行時自動裝好需要的一切,接上**免費**的開源模型(或你自己的 Claude 帳號),然後**實際執行**你的要求 —— 再即時把它做了什麼顯示給你看。這就是「只會回答的助手」和「真的把事情做完的同事」之間的差別。

---

## 主要功能

- 🎯 **一個全域快捷鍵,講人話就好。** 在任何地方按 `Alt+H` → 跳出 PowerToys 風格的輸入框。打字或用講的說出需求,按 Enter。
- 🆓 **預設免費 —— 免 API key。** 跑在 Ollama 的雲端開源模型上;唯一的設定是到 ollama.com 免費登入一次。或切換成用**你自己的 Anthropic Claude 帳號**,享受頂級品質。
- 🤖 **它會動手做,不只是描述。** 整理檔案、改名、搜尋、開程式 —— AI 直接操作你的電腦(安全地;見 [隱私與安全](#隱私與安全))。
- 📡 **結果即時串流回來。** 在輸入面板裡即時看 AI 思考與動作 —— 工具呼叫和最終答案,邊跑邊顯示。
- 🎙️ **語音輸入**(`Alt+J`)—— 用說的取代打字。📸 **截圖當情境**(`Alt+K`)—— 框選螢幕一塊區域,立刻變成附件,於是你可以問「這個錯誤是什麼?」。
- ✅ **會記住的任務佇列。** 一次丟好幾個需求,它們依序執行。完成的任務變成一份整齊的待辦清單讓你逐筆打勾 —— 點開任一筆還能回看它做了什麼。
- 🪟 **原生、輕量、漂亮。** 毛玻璃介面、約 3 MB 安裝包、不是 Electron。繁體中文 + English。
- 🔒 **隱私為本。** 零遙測。一切在本機執行;不收集任何關於你使用情形的資料。

---

## 為什麼選 FreeCowork

|  | 命令列 AI agent | 聊天助手(網頁) | **FreeCowork** |
|---|---|---|---|
| **設定** | API key + 終端機 + 設定檔 | 帳號,通常要訂閱 | **免費登入一次,零設定** |
| **怎麼下指令** | 打指令 | 在瀏覽器分頁打字 | **快捷鍵 + 自然語言 / 語音** |
| **會在你電腦上動手嗎?** | 會,但要手動且技術性高 | 不會 —— 只能聊 | **會,自動執行** |
| **起步成本** | 按 token 計費的 API | 月訂閱 | **免費額度**(或自帶帳號) |
| **對非技術使用者** | ❌ 搆不著 | ⚠️ 會講不會做 | ✅ **無腦上手** |
| **體積** | — | 瀏覽器 | **約 3 MB 原生系統匣 app** |

**切入點:** 這幾個裡面,FreeCowork 是唯一一個非技術使用者能在兩分鐘內裝好並用起來的 —— *而且*是唯一一個真的會在他們電腦上把事情做完的。

---

## 運作方式

```
┌──────────────────────────────────────────────────────────────┐
│  系統匣(常駐)                                                 │
│        │  Alt+H                                                │
│        ▼                                                       │
│  ┌─────────────────────────────┐   自然語言需求                 │
│  │  輸入面板 (Svelte 5 + 毛玻璃) │ ──────────────┐               │
│  └─────────────────────────────┘               │               │
│        ▲ 即時 stream-json(結果、工具呼叫)       │               │
│        │                                        ▼               │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │  Rust 核心 (Tauri v2):任務佇列 · 程序管理 · IPC           │ │
│  └──────────────────────────────────────────────────────────┘ │
│        │ spawn                                                  │
│        ▼                                                        │
│   Claude Code  ──►  ┌─ 免費 Ollama 雲端開源模型                  │
│                     └─ 或你自己的 Anthropic Claude 帳號          │
└──────────────────────────────────────────────────────────────┘
```

首次執行時,App 會靜默安裝 **Ollama** 與 **Claude Code**(免系統管理員),引導你到 ollama.com 用瀏覽器登入一次,然後把每個需求都交給 Claude Code 執行 —— 指向 Ollama 的免費雲端模型,或(若你選擇)你自己的 Claude 帳號。輸出以結構化的 `stream-json` 事件流回傳,面板即時呈現。

---

## 安裝

> **首次提醒:** 安裝包尚未做程式碼簽章,Windows SmartScreen 可能會警告 → 點「**更多資訊 → 仍要執行**」。

**1. 直接下載安裝檔(最簡單)**
到 [Releases 頁面](https://github.com/jaylooloomi/FreeCowork/releases/latest) 下載最新的 `*-setup.exe`,雙擊執行(免系統管理員)。

**2. 一行指令安裝(PowerShell)**
```powershell
irm https://raw.githubusercontent.com/jaylooloomi/FreeCowork/main/install.ps1 | iex
```

**3. winget** *(規劃中)*
```powershell
winget install jaylooloomi.FreeCowork
```

---

## 使用方式

1. 安裝後,FreeCowork 常駐系統匣,並隨 Windows 開機啟動。
2. 按 **`Alt+H`** 叫出輸入框。
3. 打字(或用講的)輸入需求,例如:
   - `幫我把桌面依類型整理成資料夾`
   - `把 Downloads 裡的 PDF 改名成以日期開頭`
   - `幫我整理最新五則科技新聞並存成一份筆記`
4. 按 **Enter**,在結果面板看它執行。

| 快捷鍵 | 作用 |
|---|---|
| `Alt+H`(全域) | 開 / 關輸入面板 |
| `Alt+J`(面板開著) | 語音輸入 |
| `Alt+K`(面板開著) | 框選截圖 → 附加 |
| `Enter` | 送出 |
| `Esc` | 關選單 / 關面板 |
| `↑` / `↓` | 翻歷史 |

**首次執行**會跑一次設定精靈(全程免系統管理員):安裝 Ollama 與 Claude Code,並帶你完成 ollama.com 瀏覽器登入,然後自動執行你的需求。

---

## 設定

系統匣圖示 → **設定**(儲存後自動關閉)。

| 選項 | 說明 |
|---|---|
| 語言 | 繁體中文(預設)/ English |
| 快捷鍵 | 預設 `Alt+H`;可改任意組合(如 `Ctrl+Alt+Space`) |
| 語音 / 截圖快捷鍵 | 預設 `Alt+J` / `Alt+K`;皆可調整 |
| 模型 | 從 Ollama 雲端即時目錄挑選,或用你的 Claude 帳號 |
| 謹慎模式 | AI 執行危險操作前先詢問,取代完全自動 |
| 背景模式 | 把結果串流進面板,而非開終端機(預設) |
| 助手個性 | 進階:自訂系統提示 |
| 工作目錄 | 任務的預設資料夾(留空 = 家目錄) |
| 開機自啟 | 登入後自動執行(預設開啟) |

---

## 隱私與安全

- **零遙測。** 不收集、不傳送任何使用資料。
- **安全的指令執行。** 每個子程序都以參數陣列方式 spawn —— 絕不經過 shell 字串插值 —— 因此沒有指令注入的破口。
- **自主程度由你決定。** 預設 AI 不逐項確認就動手(`--dangerously-skip-permissions`),所以真的很省事;開啟**謹慎模式**即可在危險操作前把關。

---

## 工程亮點

打造得小、快、正確 —— 不是把瀏覽器包一層的殼:

- **Tauri v2** —— Rust 核心 + **Svelte 5(runes)** 前端、WebView2。安裝包 **約 3 MB**,不是 Electron。
- **併發安全的任務佇列** —— 單一事實來源的狀態機,搭配中毒容忍鎖,panic 也絕不會讓佇列死鎖。
- **即時 `stream-json` 管線** —— 逐行解析子程序 stdout 串流到 UI;失敗會從 log 尾巴分類(訂閱 / 額度 / 認證)。
- **紮實的 Windows 整合** —— 全域快捷鍵、毛玻璃 vibrancy、以 AppUserModelID 處理的 toast 通知圖示、NSIS 每使用者安裝、開機自啟。
- **有測試、有審查** —— 114 條 Rust 單元測試;前端型別檢查;改動皆經對抗式審查強化。

---

## 開發藍圖

- [ ] 程式碼簽章(消除 SmartScreen 警告)
- [ ] winget 上架
- [ ] 自動更新(Tauri updater)
- [ ] macOS / Linux

## 已知限制

- **免費額度。** Ollama 免費雲端有 GPU 時間配額;大量使用可能需等待重置(綁帳號,換模型無效)。
- **開源模型品質。** 免費路徑用的是 Ollama 雲端開源模型,非 Anthropic 商業版 Claude —— 能力與穩定性有差。要頂級品質請用自己的 Claude 帳號。
- **目前僅支援 Windows**(10 22H2+ / 11)。
- **尚無自動更新** —— 升級請重新下載。
- 在新的工作目錄裡**第一次**執行任務,會觸發 Claude Code 自己的資料夾信任確認一次(按 Enter 即可;這是 Claude Code 的安全機制,本工具不繞過)。

---

## 開發

```
launcher/
├── src/            ← 前端 (Svelte 5 + TypeScript)
│   └── lib/        ← UI 元件與 API 包裝
└── src-tauri/      ← Rust 後端
    └── src/        ← 各功能模組
```

```powershell
cd launcher && npm install      # 安裝前端依賴
npm run tauri dev               # 開發模式(熱重載)
cd src-tauri && cargo test      # Rust 單元測試
npm run tauri build             # 產生正式安裝包 → src-tauri/target/release/bundle/nsis/
```

## 系統需求

- Windows 10 22H2+ 或 Windows 11
- 網路連線(首次安裝與雲端模型呼叫皆需要)

---

## 免責聲明

FreeCowork 是一個獨立的開源專案,**與 Anthropic 或 Ollama 無關,亦未經其背書或贊助**。「Claude」與「Claude Code」是 Anthropic 的商標;「Ollama」是其各自擁有者的商標。本工具是在你自己的帳號 / 用量下協調這些產品運作。

## 授權

MIT
