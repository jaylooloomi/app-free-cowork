# 事件紀錄:開發機臨時埠耗盡(2026-06-11)

## 摘要

最終回歸 E2E(`scripts/e2e-local.ps1`)失敗,**根因是開發機環境,不是程式回歸**:
機器上有程序以 ~96 連線/秒 對 DNS 伺服器(port 53,TCP)瘋狂連線,
累積 **23,000+ 個 TIME_WAIT**,吃光整個動態埠範圍(預設 49152 起共 16,384 個),
導致全機任何新的對外 TCP 連線(包含連到本機 `127.0.0.1:11434` 的 Ollama)都拿到
`WSAEADDRINUSE`(「一次只能用一個通訊端位址」)。

## 證據鏈

1. T15 的同一支 E2E 在前一晚**連續通過兩次**(真實雲端模型完成檔案分類)。
2. 失敗當下,`ollama list`(官方 CLI,與本專案無關)同樣連線失敗、錯誤訊息相同。
3. 執行紀錄 `fcc-20260611-092030.log` 顯示 `ollama launch` 自己嘗試啟動伺服器時 bind 失敗。
4. `Get-NetTCPConnection`:23,245 筆 TIME_WAIT,主要指向 DNS(2001:4546:1::1、61.31.1.1、61.31.233.1 的 port 53)
   與本機 `127.0.0.1:49350`(listener = Intel `esrv_svc`)。
5. 動態埠範圍正常(49152 + 16384)、11434 不在排除區間 — 排除埠保留問題。

## 可疑製造者(未定罪,留給使用者調查)

- `vpnserver_x64`(SoftEther VPN Server)— SecureNAT 的 DNS 轉送以 TCP 高頻查詢是已知模式
- Intel `esrv_svc`(Energy Server,49350 的 listener,3,901 筆 TIME_WAIT 指向它)

## 恢復方式(擇一)

1. **重開機**(最簡單;TIME_WAIT 全清。若風暴程序隨開機重啟,問題會復發 — 屆時調查上述嫌疑程序)
2. 以系統管理員執行:`netsh int ipv4 set dynamicport tcp start=1025 num=64510`
   (微軟官方的埠耗盡補救;立即生效、可逆:`... start=49152 num=16384` 還原)
3. 找出並停掉 DNS 風暴程序(`Get-NetTCPConnection -RemotePort 53` 高頻採樣,或 admin 權限 `netstat -anob`)

## 恢復後的驗證

```powershell
pwsh -NoProfile -File scripts\e2e-local.ps1   # 期望 PASS
```

## 本事件帶來的產品改進(已落版)

`ensure_server` 原本每次 ping 失敗就 spawn 一個 `ollama serve`,在壅塞環境下
會堆疊殭屍程序、把狀況越弄越糟。已加入 **30 秒 spawn 冷卻閘門**(process-global,
doctor 檢查與精靈步驟共用),並把 Degraded 訊息改為可行動的
「Ollama 服務未回應,請重新啟動 Ollama 後再試」。commit:`fix: rate-limit ollama serve spawns`。
