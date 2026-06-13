# =============================================
#   Free Claude Code 移除腳本 (Windows)
#   用途：清除整套安裝（測試環境重置用），方便重複驗證安裝流程
#   參數：-PurgeModels  一併刪除 %USERPROFILE%\.ollama（已下載的模型與金鑰）
# =============================================
[CmdletBinding()]
param(
    [switch]$PurgeModels
)

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

# 從 HKCU Uninstall 機碼依 DisplayName 找解除安裝指令（皆為免系統管理員的 per-user 安裝）
function Get-UninstallString($displayName) {
    $roots = @("HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall")
    foreach ($root in $roots) {
        foreach ($key in @(Get-ChildItem $root -ErrorAction SilentlyContinue)) {
            $p = Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue
            if ($p.DisplayName -like $displayName -and $p.UninstallString) {
                return $p.UninstallString
            }
        }
    }
    return $null
}

# UninstallString 可能帶引號："C:\...\uninstall.exe" → 取出純路徑
function Get-ExePathFromUninstallString($str) {
    if ($str -match '^"([^"]+)"') { return $Matches[1] }
    return $str
}

Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor Red
Write-Host "║   Free Claude Code 移除工具           ║" -ForegroundColor Red
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor Red
Write-Host ""

# ── 1. Free Claude Code 啟動器（Tauri / NSIS） ───────────────────────────────
Write-Step 1 4 "Free Claude Code 啟動器"

Write-Info "停止 launcher.exe..."
Stop-Process -Name "launcher" -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$fccUninst = Get-UninstallString "Free Claude Code"
if ($fccUninst) {
    $exe = Get-ExePathFromUninstallString $fccUninst
    if (Test-Path $exe) {
        Write-Info "執行 NSIS 解除安裝程式（靜默）..."
        Start-Process -FilePath $exe -ArgumentList "/S" -Wait
        Start-Sleep -Seconds 2   # NSIS 解除安裝程式會複製自身到 TEMP 後立即返回
        Write-OK "Free Claude Code 已移除"
    } else {
        Write-Skip "解除安裝程式 ($exe)"
    }
} else {
    Write-Skip "Free Claude Code（登錄檔無安裝紀錄）"
}

Write-Info "清除應用程式資料 (%APPDATA%\free-claude-code)..."
$appData = Join-Path $env:APPDATA "free-claude-code"
if (Test-Path $appData) {
    Remove-Item -Path $appData -Recurse -Force
    Write-OK "應用程式資料已清除 ($appData)"
} else {
    Write-Skip "應用程式資料"
}

# ── 2. Claude Code（原生安裝版） ──────────────────────────────────────────────
Write-Step 2 4 "Claude Code"

$claudeExe = Join-Path $env:USERPROFILE ".local\bin\claude.exe"
if (Test-Path $claudeExe) {
    Write-Info "移除 $claudeExe ..."
    Remove-Item -Path $claudeExe -Force
    Write-OK "Claude Code 已移除"
} else {
    Write-Skip "Claude Code ($claudeExe)"
}
Write-Info "注意:%USERPROFILE%\.claude 設定資料夾保留不動(登入狀態與個人設定)。"

# ── 3. Ollama ─────────────────────────────────────────────────────────────────
Write-Step 3 4 "Ollama"

$ollamaRemoved = $false
Write-Info "停止 Ollama 程序..."
Stop-Process -Name "ollama" -Force -ErrorAction SilentlyContinue
Stop-Process -Name "ollama app" -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

if (Test-Command "winget") {
    Write-Info "用 winget 移除 Ollama..."
    winget uninstall --id Ollama.Ollama --accept-source-agreements 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { $ollamaRemoved = $true; Write-OK "Ollama 已移除 (winget)" }
}
if (-not $ollamaRemoved) {
    $ollamaUninst = Get-UninstallString "Ollama*"
    if ($ollamaUninst) {
        $exe = Get-ExePathFromUninstallString $ollamaUninst
        if (Test-Path $exe) {
            Write-Info "執行 Ollama 解除安裝程式（靜默）..."
            Start-Process -FilePath $exe -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES" -Wait
            $ollamaRemoved = $true
            Write-OK "Ollama 已移除（解除安裝程式）"
        }
    }
}
if (-not $ollamaRemoved) { Write-Skip "Ollama" }

# ── 4. Ollama 模型資料（選用） ────────────────────────────────────────────────
Write-Step 4 4 "Ollama 模型資料"

$ollamaData = Join-Path $env:USERPROFILE ".ollama"
if ($PurgeModels) {
    if (Test-Path $ollamaData) {
        Write-Info "清除 $ollamaData（含已下載模型與帳號金鑰）..."
        Remove-Item -Path $ollamaData -Recurse -Force
        Write-OK "Ollama 資料夾已清除"
    } else {
        Write-Skip "Ollama 資料夾"
    }
} else {
    Write-Info "保留 $ollamaData(模型與帳號金鑰)。要一併刪除請加 -PurgeModels。"
}

# ── 完成 ──────────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "╔══════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║   移除完成！                          ║" -ForegroundColor Green
Write-Host "╚══════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""
Write-Info "重新安裝請執行："
Write-Host "  irm https://raw.githubusercontent.com/jaylooloomi/free-claude-code/main/install.ps1 | iex" -ForegroundColor Cyan
Write-Host ""
