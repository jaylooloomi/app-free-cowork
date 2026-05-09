# free-cloud-models

一鍵安裝 Python、Node.js、Ollama 雲端模型，並啟動 Claude Code — 全程免費，不需要 Anthropic API Key。

---

## 快速開始（Windows）

打開 **PowerShell**，貼上這一行：

```powershell
irm https://raw.githubusercontent.com/jaylooloomi/free-cloud-models/main/setup.ps1 | iex
```

就這樣。腳本會自動完成以下所有步驟：

---

## 安裝流程

```
[1/5] Python      → 偵測是否已安裝，未安裝則自動安裝
[2/5] Node.js     → 偵測是否已安裝，未安裝則自動安裝
[3/5] Ollama      → 偵測是否已安裝，未安裝則自動安裝
[4/5] 雲端模型    → 掃描 ollama.com 所有 cloud 模型並下載
[5/5] Claude Code → 偵測是否已安裝，未安裝則自動安裝
      ↓
  ollama launch claude（啟動，可選擇要使用的模型）
```

> 已安裝的項目會自動跳過，不會重複安裝。

---

## 系統需求

- Windows 10 / 11
- PowerShell 5.1 以上
- 網路連線
- `winget`（Windows 11 內建；Windows 10 可至 Microsoft Store 安裝 [App Installer](https://apps.microsoft.com/detail/9NBLGGH4NNS1)）

---

## 專案結構

```
free-cloud-models/
├── setup.ps1                  # Windows 一鍵安裝腳本
└── pull_ollama_cloud_model.py # 掃描並下載所有 Ollama 雲端模型
```

---

## 常見問題

**Q：需要 Anthropic API Key 嗎？**
不需要。本專案透過 `ollama launch claude` 讓 Claude Code 使用本地 / 雲端開源模型，完全免費。

**Q：安裝後要多少磁碟空間？**
取決於下載的模型數量。單一雲端模型通常不佔本地空間（透過 Ollama 雲端執行）。

**Q：安裝過程卡住怎麼辦？**
重新開啟 PowerShell 再執行一次，腳本會跳過已安裝的項目，從中斷點繼續。

**Q：只想跑模型安裝腳本？**
```powershell
python pull_ollama_cloud_model.py
```
