# =============================================
#   free-claude-code 移除腳本 (Windows)
#   用途：清除所有安裝，方便重複驗證安裝流程
# =============================================

$ErrorActionPreference = "Continue"

function Write-Step($step, $total, $msg) {
    Write-Host ""
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Red
    Write-Host "  [$step/$total] 移除 $msg" -ForegroundColor Red
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Red
}

function Write-OK($msg)   { Write-Host "  [OK] $msg" -ForegroundColor Green }
function Write-Skip($msg) { Write-Host "  [跳過] $msg 不存在，略過。" -ForegroundColor Yellow }
function Write-Info($msg) { Write-Host "  [*] $msg" -ForegroundColor White }

function Test-Command($cmd) {
    return $null -ne (Get-Command $cmd -ErrorAction SilentlyContinue)
}

Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor Red
Write-Host "║   free-claude-code 移除工具           ║" -ForegroundColor Red
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor Red
Write-Host ""

# ── 1. Claude Code ────────────────────────────────────────────────────────────
Write-Step 1 4 "Claude Code"

if (Test-Command "npm") {
    Write-Info "移除 Claude Code npm 套件..."
    npm uninstall -g @anthropic-ai/claude-code 2>&1
    Write-OK "Claude Code 已移除"
} else {
    Write-Skip "Claude Code"
}

# ── 2. Ollama ─────────────────────────────────────────────────────────────────
Write-Step 2 4 "Ollama"

if (Test-Command "ollama") {
    Write-Info "停止 Ollama 服務..."
    Stop-Process -Name "ollama" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2

    Write-Info "用 winget 移除 Ollama..."
    winget uninstall --id Ollama.Ollama --accept-source-agreements 2>&1

    Write-Info "清除 Ollama 模型與資料..."
    $ollamaData = "$env:USERPROFILE\.ollama"
    if (Test-Path $ollamaData) {
        Remove-Item -Path $ollamaData -Recurse -Force
        Write-OK "Ollama 資料夾已清除 ($ollamaData)"
    }
    Write-OK "Ollama 已移除"
} else {
    Write-Skip "Ollama"
}

# ── 3. Node.js ────────────────────────────────────────────────────────────────
Write-Step 3 4 "Node.js"

if (Test-Command "node") {
    Write-Info "用 winget 移除 Node.js..."
    winget uninstall --id OpenJS.NodeJS.LTS --accept-source-agreements 2>&1
    Write-OK "Node.js 已移除"
} else {
    Write-Skip "Node.js"
}

# ── 4. Python ─────────────────────────────────────────────────────────────────
Write-Step 4 4 "Python"

if (Test-Command "python") {
    Write-Info "用 winget 移除 Python..."
    winget uninstall --id Python.Python.3 --accept-source-agreements 2>&1
    Write-OK "Python 已移除"
} else {
    Write-Skip "Python"
}

# ── 完成 ──────────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║   移除完成！請重新開啟終端機。        ║" -ForegroundColor Green
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Info "重新安裝請執行："
Write-Host "  irm https://raw.githubusercontent.com/jaylooloomi/free-claude-code/main/setup.ps1 | iex" -ForegroundColor Cyan
Write-Host ""
