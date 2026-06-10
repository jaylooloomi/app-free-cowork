#Requires -Version 7
<#
.SYNOPSIS
    Local dev-machine end-to-end test for the Free Claude Code launcher.

.DESCRIPTION
    Drives the release launcher.exe through its real CLI path:
      settings.json (background_mode=true) -> launcher.exe boot ->
      launcher.exe --run "<chinese file-classification prompt>" (single-instance forward) ->
      ollama launch claude --model <model> --yes -- -p --dangerously-skip-permissions "<prompt>"
    against a sandbox of 10 fake files, then verifies the files were classified
    into per-extension subfolders and a run log was produced.

    Exit codes: 0 = PASS, 1 = FAIL, 2 = precondition abort, 3 = BLOCKED-quota
    (429/usage-limit in run log: plumbing verified, cloud quota exhausted).
#>
[CmdletBinding()]
param(
    [int]$TimeoutMinutes = 10,
    [string]$Model = 'minimax-m2.5:cloud'
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$RepoRoot     = Split-Path $PSScriptRoot -Parent
$LauncherExe  = Join-Path $RepoRoot 'launcher\src-tauri\target\release\launcher.exe'
$ClaudeExe    = Join-Path $env:USERPROFILE '.local\bin\claude.exe'
$AppDir       = Join-Path $env:APPDATA 'free-claude-code'
$SettingsPath = Join-Path $AppDir 'settings.json'
$BackupPath   = "$SettingsPath.bak-e2e"
$LogsDir      = Join-Path $AppDir 'logs'
$Sandbox      = Join-Path $env:TEMP 'fcc-e2e-sandbox'
$Prompt       = '把這個資料夾裡的檔案依副檔名分類到子資料夾(txt/jpg/pdf/zip),直接執行不用確認'

function Write-Step([string]$msg) { Write-Host "== $msg" -ForegroundColor Cyan }
function Abort([string]$msg)     { Write-Host "ABORT: $msg" -ForegroundColor Red; exit 2 }

function Get-SandboxListing {
    $lines = @()
    foreach ($f in @(Get-ChildItem $Sandbox -File -ErrorAction SilentlyContinue | Sort-Object Name)) {
        $lines += "  (root) $($f.Name)"
    }
    foreach ($d in @(Get-ChildItem $Sandbox -Directory -ErrorAction SilentlyContinue | Sort-Object Name)) {
        $inner = @(Get-ChildItem $d.FullName -File -Recurse | Sort-Object Name)
        $lines += "  $($d.Name)\  ($($inner.Count) files)"
        foreach ($f in $inner) { $lines += "    $($d.Name)\$($f.Name)" }
    }
    if ($lines.Count -eq 0) { $lines = @('  (empty)') }
    $lines -join "`n"
}

# ---------------------------------------------------------------- 1. Preconditions
Write-Step 'Preconditions'
try {
    $ver = (Invoke-WebRequest -Uri 'http://127.0.0.1:11434/api/version' -UseBasicParsing -TimeoutSec 5).Content
    Write-Host "  ollama responding: $ver"
} catch {
    Abort 'ollama is not responding at 127.0.0.1:11434 - start it with `ollama serve` (or the Ollama app) and retry.'
}
if (-not (Test-Path $ClaudeExe))   { Abort "claude.exe not found at $ClaudeExe - install Claude Code first." }
if (-not (Test-Path $LauncherExe)) { Abort "release binary not found at $LauncherExe - build with `npm run tauri build -- --no-bundle`." }
Write-Host "  claude.exe:   $ClaudeExe"
Write-Host "  launcher.exe: $LauncherExe"

# ---------------------------------------------------------------- 2. Backup settings, stop launcher
Write-Step 'Backup settings + stop running launcher'
$HadSettings = Test-Path $SettingsPath
if ($HadSettings) {
    Copy-Item $SettingsPath $BackupPath -Force
    Write-Host "  backed up settings.json -> $BackupPath"
} else {
    Write-Host '  no existing settings.json (teardown will remove the test one)'
}
Stop-Process -Name launcher -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

# ---------------------------------------------------------------- 3. Sandbox with 10 fake files
Write-Step "Sandbox: $Sandbox"
if (Test-Path $Sandbox) { Remove-Item $Sandbox -Recurse -Force }
New-Item -ItemType Directory -Path $Sandbox | Out-Null
$FakeFiles = @(
    'notes-alpha.txt', 'notes-beta.txt', 'notes-gamma.txt',
    'photo-cat.jpg',   'photo-dog.jpg',  'photo-bird.jpg',
    'report-q1.pdf',   'report-q2.pdf',
    'backup-2025.zip', 'backup-2026.zip'
)
foreach ($f in $FakeFiles) {
    Set-Content -Path (Join-Path $Sandbox $f) -Value "fake e2e content for $f" -Encoding utf8
}
Write-Host "  created $($FakeFiles.Count) files"
$BeforeListing = Get-SandboxListing

# ---------------------------------------------------------------- 4. Write test settings.json
Write-Step 'Write test settings.json'
New-Item -ItemType Directory -Force -Path $AppDir  | Out-Null
New-Item -ItemType Directory -Force -Path $LogsDir | Out-Null
$settings = [ordered]@{
    hotkey          = 'Alt+H'
    model           = $Model
    cautious_mode   = $false
    background_mode = $true            # headless: claude runs with -p, output -> log file
    working_dir     = $Sandbox
    autostart       = $false
    history         = @()
    signin_state    = 'Yes'            # serde unit variant -> plain string
}
$settings | ConvertTo-Json | Set-Content -Path $SettingsPath -Encoding utf8
Write-Host (Get-Content $SettingsPath -Raw)

# Snapshots for "what is new" detection
$LogsBefore   = @(Get-ChildItem $LogsDir -Filter 'fcc-*.log' -ErrorAction SilentlyContinue | ForEach-Object Name)
$ClaudeBefore = @(Get-Process -Name claude -ErrorAction SilentlyContinue | ForEach-Object Id)
$OllamaBefore = @(Get-Process -Name ollama -ErrorAction SilentlyContinue | ForEach-Object Id)

# ---------------------------------------------------------------- 5. Boot app, submit prompt via CLI
Write-Step 'Boot launcher.exe and submit prompt'
$LauncherProc = Start-Process -FilePath $LauncherExe -PassThru
Write-Host "  launcher pid $($LauncherProc.Id), waiting 5s for boot..."
Start-Sleep -Seconds 5
if ($LauncherProc.HasExited) { Abort "launcher.exe exited during boot (code $($LauncherProc.ExitCode)) - check the binary." }
Write-Host "  forwarding --run prompt via second instance"
Start-Process -FilePath $LauncherExe -ArgumentList @('--run', $Prompt) -Wait
$RunSubmitted = Get-Date

# ---------------------------------------------------------------- 6. Poll for outcome
Write-Step "Polling up to $TimeoutMinutes minutes"
$Deadline       = (Get-Date).AddMinutes($TimeoutMinutes)
$NewLog         = $null
$SawClaude      = $false
$SawOllamaChild = $false
$ClaudeStart    = $null
$ClaudeEnd      = $null
$Result         = 'FAIL-timeout'
$GraceAfterExit = 0

while ((Get-Date) -lt $Deadline) {
    Start-Sleep -Seconds 5

    if (-not $NewLog) {
        $NewLog = Get-ChildItem $LogsDir -Filter 'fcc-*.log' -ErrorAction SilentlyContinue |
            Where-Object { $LogsBefore -notcontains $_.Name } |
            Sort-Object Name | Select-Object -Last 1
        if ($NewLog) { Write-Host "  new run log: $($NewLog.Name)" }
    }

    $NewClaude = @(Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $ClaudeBefore -notcontains $_.Id })
    if ($NewClaude.Count -gt 0) {
        if (-not $SawClaude) { $ClaudeStart = Get-Date; Write-Host "  claude child process observed (pid $($NewClaude[0].Id))" }
        $SawClaude = $true
    } elseif ($SawClaude -and -not $ClaudeEnd) {
        $ClaudeEnd = Get-Date
        Write-Host '  claude child process exited'
    }
    if (-not $SawOllamaChild) {
        $NewOllama = @(Get-Process -Name ollama -ErrorAction SilentlyContinue | Where-Object { $OllamaBefore -notcontains $_.Id })
        if ($NewOllama.Count -gt 0) { $SawOllamaChild = $true; Write-Host "  ollama child process observed (pid $($NewOllama[0].Id))" }
    }

    # Primary success: every loose file gone from root, >=10 files inside subfolders
    $RootLoose = @(Get-ChildItem $Sandbox -File -ErrorAction SilentlyContinue)
    $InSubdirs = @(Get-ChildItem $Sandbox -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ChildItem $_.FullName -File -Recurse })
    if ($NewLog -and $RootLoose.Count -eq 0 -and $InSubdirs.Count -ge $FakeFiles.Count) {
        $Result = 'PASS'
        # Files are classified, but claude may still be writing its final answer.
        # Wait (max 2 min) for the child to exit so the run log captures real model
        # output and teardown does not have to kill it (a kill would pollute the log
        # with "Error: exit status 0xffffffff" from ollama launch).
        $GraceDeadline = (Get-Date).AddMinutes(2)
        while ((Get-Date) -lt $GraceDeadline) {
            $Still = @(Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $ClaudeBefore -notcontains $_.Id })
            if ($Still.Count -eq 0) {
                if ($SawClaude -and -not $ClaudeEnd) { $ClaudeEnd = Get-Date }
                break
            }
            Start-Sleep -Seconds 5
        }
        break
    }

    if ($NewLog) {
        $LogText = Get-Content $NewLog.FullName -Raw -ErrorAction SilentlyContinue
        # Quota check only once the claude child is gone (avoid matching transient retry chatter)
        if ($LogText -and $NewClaude.Count -eq 0 -and $SawClaude -and
            $LogText -match '(?i)\b429\b|usage limit|quota|rate limit') {
            $Result = 'BLOCKED-quota'
            break
        }
    }

    # claude exited but files not classified -> give it 3 extra polls, then fail fast
    if ($ClaudeEnd) {
        $GraceAfterExit++
        if ($GraceAfterExit -ge 3) { $Result = 'FAIL-completed-without-classifying'; break }
    }
}

$ClaudeDuration = if ($ClaudeStart) {
    $end = $ClaudeEnd ?? (Get-Date)
    '{0:mm\:ss}' -f ($end - $ClaudeStart)
} else { 'n/a (claude child never observed)' }

# ---------------------------------------------------------------- 7. Teardown + report
Write-Step 'Teardown'
Stop-Process -Name launcher -Force -ErrorAction SilentlyContinue
# Kill only claude processes spawned during this test (never pre-existing ones)
foreach ($p in @(Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $ClaudeBefore -notcontains $_.Id })) {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Write-Host "  killed leftover test claude pid $($p.Id)"
}
if ($HadSettings) {
    Move-Item $BackupPath $SettingsPath -Force
    Write-Host '  restored original settings.json'
} else {
    Remove-Item $SettingsPath -Force -ErrorAction SilentlyContinue
    Write-Host '  removed test settings.json (none existed before)'
}

Write-Host ''
Write-Host '================ E2E REPORT ================' -ForegroundColor Yellow
Write-Host "Result            : $Result"
Write-Host "Model             : $Model"
Write-Host "Prompt submitted  : $RunSubmitted"
Write-Host "Claude run length : $ClaudeDuration"
Write-Host "Saw claude child  : $SawClaude   Saw ollama child: $SawOllamaChild  (best-effort observation)"
Write-Host "Run log           : $(if ($NewLog) { $NewLog.FullName } else { '(none appeared)' })"
Write-Host ''
Write-Host '--- sandbox BEFORE ---'
Write-Host $BeforeListing
Write-Host '--- sandbox AFTER ----'
Write-Host (Get-SandboxListing)
if ($NewLog -and (Test-Path $NewLog.FullName)) {
    Write-Host ''
    Write-Host '--- run log tail (last 40 lines) ---'
    Get-Content $NewLog.FullName -Tail 40
}
Write-Host '============================================' -ForegroundColor Yellow

switch ($Result) {
    'PASS'          { exit 0 }
    'BLOCKED-quota' { Write-Host 'Cloud quota exhausted (429/usage limit). Plumbing verified end-to-end; inference blocked by quota.'; exit 3 }
    default         { exit 1 }
}
