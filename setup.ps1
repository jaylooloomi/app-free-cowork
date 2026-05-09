# =============================================
#   free-cloud-models 一鍵安裝腳本 (Windows)
#   使用方式：
#   irm https://raw.githubusercontent.com/jaylooloomi/free-cloud-models/main/setup.ps1 | iex
# =============================================

$ErrorActionPreference = "Continue"

function Write-Step($step, $total, $msg) {
    Write-Host ""
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Cyan
    Write-Host "  [$step/$total] $msg" -ForegroundColor Cyan
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Cyan
}

function Write-OK($msg)         { Write-Host "  [OK] $msg" -ForegroundColor Green }
function Write-Skip($msg)       { Write-Host "  [跳過] $msg 已安裝，略過。" -ForegroundColor Yellow }
function Write-Info($msg)       { Write-Host "  [*] $msg" -ForegroundColor White }
function Write-Err($msg)        { Write-Host "  [錯誤] $msg" -ForegroundColor Red }

function Refresh-Path {
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                [System.Environment]::GetEnvironmentVariable("Path", "User")
}

function Test-Command($cmd) {
    return $null -ne (Get-Command $cmd -ErrorAction SilentlyContinue)
}

# ══════════════════════════════════════════
Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor Magenta
Write-Host "║   free-cloud-models 一鍵安裝工具     ║" -ForegroundColor Magenta
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor Magenta
Write-Host ""

# ── 1. Python ─────────────────────────────────────────────────────────────────
Write-Step 1 5 "Python"

if (Test-Command "python") {
    $v = python --version 2>&1
    Write-Skip "Python  ($v)"
} else {
    Write-Info "找不到 Python，使用 winget 安裝..."
    winget install -e --id Python.Python.3 --accept-source-agreements --accept-package-agreements
    Refresh-Path
    if (Test-Command "python") {
        Write-OK "Python 安裝完成"
    } else {
        Write-Err "Python 安裝後仍無法偵測到，請重新開啟終端機再試。"
        exit 1
    }
}

# ── 2. Node.js ────────────────────────────────────────────────────────────────
Write-Step 2 5 "Node.js"

if (Test-Command "node") {
    $v = node --version
    Write-Skip "Node.js ($v)"
} else {
    Write-Info "找不到 Node.js，使用 winget 安裝..."
    winget install -e --id OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements
    Refresh-Path
    if (Test-Command "node") {
        Write-OK "Node.js 安裝完成"
    } else {
        Write-Err "Node.js 安裝後仍無法偵測到，請重新開啟終端機再試。"
        exit 1
    }
}

# ── 3. Ollama ─────────────────────────────────────────────────────────────────
Write-Step 3 5 "Ollama"

if (Test-Command "ollama") {
    $v = ollama --version 2>&1
    Write-Skip "Ollama  ($v)"
} else {
    Write-Info "找不到 Ollama，使用 winget 安裝..."
    winget install -e --id Ollama.Ollama --accept-source-agreements --accept-package-agreements
    Refresh-Path
    if (Test-Command "ollama") {
        Write-OK "Ollama 安裝完成"
    } else {
        Write-Err "Ollama 安裝後仍無法偵測到，請重新開啟終端機再試。"
        exit 1
    }
}

# 確保 Ollama 服務正在執行
Write-Info "確認 Ollama 服務狀態..."
$ollamaProcess = Get-Process -Name "ollama" -ErrorAction SilentlyContinue
if (-not $ollamaProcess) {
    Write-Info "啟動 Ollama 背景服務..."
    Start-Process "ollama" -ArgumentList "serve" -WindowStyle Hidden
    Start-Sleep -Seconds 3
    Write-OK "Ollama 服務已啟動"
} else {
    Write-Skip "Ollama 服務"
}

# ── 4. 拉取雲端模型 ───────────────────────────────────────────────────────────
Write-Step 4 5 "安裝 Ollama 雲端模型"

$pyScript = "$env:TEMP\pull_ollama_cloud_model.py"
Write-Info "下載模型安裝腳本..."
try {
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/jaylooloomi/free-cloud-models/main/pull_ollama_cloud_model.py" -OutFile $pyScript -UseBasicParsing
    Write-Info "執行模型安裝（這可能需要數分鐘，請耐心等候）..."
    python $pyScript
    Write-OK "雲端模型安裝完成"
} catch {
    Write-Err "無法下載或執行模型安裝腳本：$_"
    exit 1
}

# ── 5. Claude Code ────────────────────────────────────────────────────────────
Write-Step 5 5 "Claude Code"

if (Test-Command "claude") {
    $v = claude --version 2>&1
    Write-Skip "Claude Code ($v)"
} else {
    Write-Info "安裝 Claude Code..."
    npm install -g @anthropic-ai/claude-code
    Refresh-Path
    if (Test-Command "claude") {
        Write-OK "Claude Code 安裝完成"
    } else {
        Write-Err "Claude Code 安裝後仍無法偵測到，請重新開啟終端機再試。"
        exit 1
    }
}

# ── 完成，啟動！ ──────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║   全部安裝完成！正在啟動 Claude...   ║" -ForegroundColor Green
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Info "執行 ollama launch claude（可用方向鍵選擇模型）"
Write-Host ""

ollama launch claude -- --dangerously-skip-permissions
