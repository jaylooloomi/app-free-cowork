# AGENTS.md

給 AI coding agent 與工程師的專案指南。**動工前請先讀「建置與執行」一節** —— 這裡記錄了幾個會讓你白白浪費一兩個小時的坑。

## 專案結構

- `launcher/` — Tauri 2 + SvelteKit 桌面 app(全域熱鍵啟動器)。前端 Svelte 5(`launcher/src`),後端 Rust(`launcher/src-tauri/src`)。
- `scripts/` — 安裝 / E2E 的 PowerShell 腳本。
- `docs/` — 設計規格(`superpowers/specs`)、計畫(`superpowers/plans`)、事件紀錄(`incident-*.md`)。

## 建置與執行(最重要,先看這裡)

### ⛔ 不要用 `cargo build` / `cargo run` 直接建這個 app

一律走官方 Tauri CLI:

```bash
# 開發(熱重載)
npm --prefix launcher run tauri dev
# 正式建置(含安裝包)
npm --prefix launcher run tauri build
# 只要正式版執行檔、不要安裝包
npm --prefix launcher run tauri build -- --no-bundle
```

**為什麼?** Tauri 用 `custom-protocol` feature 決定前端從哪裡載入:

- `tauri` crate 的 build script:`let dev = !custom_protocol;`(見 `tauri-2.x/build.rs`,會 emit `cargo:dev=...`)。
- 啟用 `custom-protocol`(`tauri build` 會自動開)→ `dev=false` → **前端內嵌進 exe**(正式版)。
- 沒啟用(`cargo build` 預設不開)→ `dev=true` → app 改去連 `http://localhost:1420`(Vite dev server)。

於是 `cargo build --release` 產出的 exe,在沒跑 dev server 的機器上會**整個 UI 載不出來**:畫面變空白 /
雲朵圖示 + 捲軸,設定視窗顯示 `localhost 拒絕連線 ERR_CONNECTION_REFUSED`。**這不是程式 bug,是建置方式錯。**

**怎麼驗證一個 build 是正式版**:檢查
`launcher/src-tauri/target/release/build/tauri-*/output` 裡的 `cargo:dev=` —— 必須是 `cargo:dev=false`。

### 工具鏈需求(Windows)

- **Rust**(rustup,host triple = `x86_64-pc-windows-msvc`)
- **MSVC C++ build tools** —— Visual Studio 的「Desktop development with C++」工作負載(提供 MSVC 工具集
  `cl.exe` / `link.exe` + Windows SDK)。只裝 VS 殼層 / 只有 .NET 工作負載**不夠**,Rust MSVC target 連結會失敗。
- **Node**(npm)
- **WebView2 Runtime**(Windows 11 內建)

用 CLI 補裝 VS C++ 工作負載時踩過的坑:

```powershell
# 需提權。用 & 呼叫運算子直接帶參數(別用 Start-Process -ArgumentList 帶含空格的路徑,會被打亂)
& "C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe" `
  modify --installPath "C:\Program Files\Microsoft Visual Studio\<ver>\Community" `
  --add Microsoft.VisualStudio.Workload.NativeDesktop --includeRecommended `
  --quiet --norestart
```

- **不要加 `--wait`** —— 這個版本的 `modify` 不吃這個選項,會直接回 exit `87`(參數錯誤),整條指令被拒。
- **不要把 `update` 和 `modify` 接連一起跑** —— 兩者的下載會互搶而被取消(`0x8013153b`「已取消作業」)。
  需要更新就先單獨跑、等它完全結束,再跑 `modify`。

### 前端必須先建好

`generate_context!()` 在**編譯期**把 `frontendDist`(`../build`)內嵌進 exe,所以 Rust 編譯前
`launcher/build` 必須存在。`tauri build` / `tauri dev` 會自動先跑 `beforeBuildCommand`(`npm run build`)
處理掉;只有你手動繞過 CLI 時才需要自己先 `npm --prefix launcher run build`。

## 在本機測試自己的 build(給 agent 的提醒)

- NSIS 安裝包以 currentUser 模式裝到 `%LOCALAPPDATA%\FreeCowork\launcher.exe`。要測本機 build,
  **覆蓋這個 exe 再重啟即可**(圖示資源已在安裝目錄、前端已內嵌)。覆蓋前先備份成 `.bak`。
- **從非互動 / service session 啟動 GUI,要用 `explorer.exe <exe 路徑>`**,讓它跑在使用者的互動桌面。
  直接 `Start-Process` 可能讓 WebView2 在沒有桌面的 window station 初始化失敗,畫面壞掉 ——
  症狀跟上面 dev-mode 那個很像,別搞混(用 `cargo:dev` 與「設定視窗是否出現 localhost 錯誤」來區分)。

## 熱鍵

主啟動熱鍵存在 `settings.json` 的 `hotkey`(預設 `Ctrl+Alt+Space`,使用者可改成如 `Alt+H`)。
所有開窗入口(全域熱鍵、托盤選單、`--show-palette`、second-instance)都共用
`show_palette_centered()`(`launcher/src-tauri/src/lib.rs`)—— 改開窗行為改這一處即可。
