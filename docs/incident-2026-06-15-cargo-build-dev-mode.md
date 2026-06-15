# 事件紀錄:本機自建執行檔 UI 全載不出來(2026-06-15)

## 摘要

在本機修改 `show_palette_centered`(讓面板開在焦點視窗所在螢幕)後,用
`cargo build --release` 自建 `launcher.exe` 覆蓋安裝,結果面板與設定視窗的 **UI 完全載不出來**:
畫面只剩雲朵圖示 + 捲軸,設定視窗顯示 `嗯…無法連線到此頁面 / localhost 拒絕連線 /
ERR_CONNECTION_REFUSED`。

**根因是建置方式,不是程式回歸**:`cargo build` 不會啟用 Tauri 的 `custom-protocol` feature,
產出的是 **dev-mode 執行檔**,前端會去連 `http://localhost:1420`(Vite dev server);該機器沒跑
dev server,於是整個前端載不出來。正解是改用官方 `tauri build`。

## 證據鏈

1. 設定視窗的錯誤畫面明確指向 `localhost`(`ERR_CONNECTION_REFUSED`)→ webview 導航到 dev server。
2. `tauri` crate build script(`tauri-2.11.2/build.rs:257`):
   ```rust
   let dev = !custom_protocol;
   println!("cargo:dev={dev}");
   ```
   `tauri-build` 再用它設 `cfg(dev)`,`tauri::is_dev()` 據此決定載 `devUrl` 還是內嵌資源。
3. 比對兩份 build script 輸出(`target/release/build/tauri-*/output`):
   - 舊 `cargo build --release` → `cargo:dev=true`(壞)
   - 改用 `tauri build` → `cargo:dev=false`(正式版)
4. 檔案大小佐證:已知能用的舊安裝檔 12.33 MB、`tauri build` 正式版 12.35 MB(都內嵌前端);
   壞掉的 `cargo build` dev 版只有 12.27 MB(沒嵌前端)。

## 正解

```powershell
# 只要正式版執行檔(不要安裝包)
npm --prefix launcher run tauri build -- --no-bundle
# 產出 launcher/src-tauri/target/release/launcher.exe(dev=false、前端內嵌)
```

覆蓋本機安裝檔(`%LOCALAPPDATA%\FreeCowork\launcher.exe`)後重啟即可。

## 額外踩到的坑(一併記錄)

- **VS C++ 工作負載 CLI 安裝**:`setup.exe modify` **不接受 `--wait`**(回 exit 87);
  且 `update` 與 `modify` 接連跑會讓下載互搶取消(`0x8013153b`)。詳見 `AGENTS.md`。
- **agent 從非互動 session 啟動 GUI**:要用 `explorer.exe <exe>`,否則 WebView2 在無桌面的
  window station 初始化失敗,症狀與本事件的 dev-mode 全白「長得很像」,容易誤判。

## 驗證

1. `target/release/build/tauri-*/output` 出現 `cargo:dev=false`。
2. 覆蓋安裝後按主熱鍵(`Alt+H`),面板正常顯示(正常輸入框、無 localhost 錯誤),
   且會出現在目前焦點視窗所在的螢幕。

## 預防(已落版)

新增 `AGENTS.md`(repo 根目錄),把「一律用 `tauri build`、不要 `cargo build`」與工具鏈/安裝坑
列為動工前必讀。
