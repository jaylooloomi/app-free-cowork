#Requires -Version 5.1
<#
.SYNOPSIS
    Windows Sandbox fresh-machine E2E harness for Free Claude Code launcher.

.DESCRIPTION
    Runs inside the Windows Sandbox (mapped via e2e-sandbox.wsb).
    Checks:
      1. NSIS installer installs the app silently; verifies install dir + exe.
      2. App process stays alive; no crash in Event Log.
      3. Bootstrap path: Ollama direct download + claude.ai/install.ps1.
      4. Second launcher --run "test" with dependencies installed.
    Writes a structured PASS/FAIL/DATA report to C:\out\report.txt.

    NOTE: This script runs in Windows Sandbox (no internet access restrictions by
    default). If Sandbox networking is disabled the bootstrap steps will FAIL with
    download errors — that is expected and will be recorded as DATA.
#>

$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$ReportPath = 'C:\out\report.txt'
$Lines      = [System.Collections.Generic.List[string]]::new()
$Stamp      = Get-Date -Format 'yyyy-MM-dd HH:mm:ss UTC'

function W([string]$line) {
    $Lines.Add($line)
    Write-Host $line
}

function WriteReport {
    # Ensure C:\out exists (mapped folder may not auto-create)
    if (-not (Test-Path 'C:\out')) { New-Item -ItemType Directory -Force 'C:\out' | Out-Null }
    $Lines | Set-Content -Path $ReportPath -Encoding UTF8
}

function Check([string]$id, [bool]$pass, [string]$evidence) {
    $status = if ($pass) { 'PASS' } else { 'FAIL' }
    W "[$status] $id"
    W "       $evidence"
    W ''
}

function Data([string]$id, [string]$value) {
    W "[DATA] $id"
    W "       $value"
    W ''
}

# ─────────────────────────────────────────────────────────────────────────────
W '=== Free Claude Code – Windows Sandbox E2E Report ==='
W "Date: $Stamp"
W "Sandbox hostname: $env:COMPUTERNAME"
W ''

# ─────────────────────────────────────────────────────────────────────────────
W '── Check 0: Winget presence (data gathering) ──'
try {
    $wv = & winget --version 2>&1
    Data 'winget-version' ($wv | Out-String).Trim()
} catch {
    Data 'winget-version' "NOT PRESENT: $_"
}

# ─────────────────────────────────────────────────────────────────────────────
W '── Check 1: NSIS installer ──'

$SetupExe = Get-ChildItem 'C:\installer\*-setup.exe' -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not $SetupExe) {
    Check 'installer-found' $false 'No *-setup.exe in C:\installer — was the NSIS bundle built? Run: cargo tauri build'
} else {
    Data 'installer-path' $SetupExe.FullName
    Data 'installer-size' "$([math]::Round($SetupExe.Length/1MB,2)) MB"

    W "Starting installer: $($SetupExe.FullName) /S"
    try {
        $proc = Start-Process -FilePath $SetupExe.FullName -ArgumentList '/S' -PassThru -Wait
        Data 'installer-exit-code' "$($proc.ExitCode)"
    } catch {
        Check 'installer-launched' $false "Start-Process failed: $_"
    }

    # Discover install location via uninstall registry key
    $uninstKey = Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue |
        Get-ItemProperty -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -like '*Free Claude Code*' } |
        Select-Object -First 1

    if ($uninstKey) {
        $InstallDir = $uninstKey.InstallLocation
        Data 'install-location-registry' $InstallDir
        Data 'uninstall-string' $uninstKey.UninstallString
        Check 'install-dir-exists' (Test-Path $InstallDir) "Path: $InstallDir"

        $LauncherExe = Join-Path $InstallDir 'launcher.exe'
        Check 'launcher-exe-exists' (Test-Path $LauncherExe) "Path: $LauncherExe"
    } else {
        # Fallback: check known NSIS currentUser location
        $FallbackDir = "$env:LOCALAPPDATA\Free Claude Code"
        Data 'install-location-fallback' $FallbackDir
        Check 'install-dir-exists-fallback' (Test-Path $FallbackDir) "Path: $FallbackDir"
        $LauncherExe = Join-Path $FallbackDir 'launcher.exe'
        $InstallDir  = $FallbackDir
        Check 'launcher-exe-exists-fallback' (Test-Path $LauncherExe) "Path: $LauncherExe"
    }
}

# ─────────────────────────────────────────────────────────────────────────────
W '── Check 2: First run – wizard/doctor route (no deps installed) ──'

$LauncherExe = if ($InstallDir -and (Test-Path (Join-Path $InstallDir 'launcher.exe'))) {
    Join-Path $InstallDir 'launcher.exe'
} elseif (Test-Path "$env:LOCALAPPDATA\Free Claude Code\launcher.exe") {
    "$env:LOCALAPPDATA\Free Claude Code\launcher.exe"
} else {
    $null
}

if (-not $LauncherExe) {
    Check 'launcher-first-run' $false 'launcher.exe not found — skipping run checks'
} else {
    # Run --run "test"; in a fresh sandbox (no ollama/claude) doctor -> wizard route
    $RunProc = $null
    try {
        $RunProc = Start-Process -FilePath $LauncherExe -ArgumentList '--run', 'test' -PassThru
        Start-Sleep -Seconds 8

        $StillAlive = -not $RunProc.HasExited
        Check 'launcher-first-run-alive' $StillAlive `
            "PID $($RunProc.Id); ExitCode=$(if ($RunProc.HasExited) { $RunProc.ExitCode } else { 'running' })"

        # Check Event Log for app errors (best-effort; Sandbox may limit access)
        try {
            $CrashEvents = Get-WinEvent -FilterHashtable @{
                LogName   = 'Application'
                Id        = 1000
                StartTime = (Get-Date).AddMinutes(-5)
            } -ErrorAction SilentlyContinue |
                Where-Object { $_.Message -like '*launcher*' }
            $CrashCount = ($CrashEvents | Measure-Object).Count
            Check 'no-crash-event-log' ($CrashCount -eq 0) "Crash events (EventID 1000) for launcher.exe: $CrashCount"
        } catch {
            Data 'event-log-check' "Could not query Event Log: $_"
        }

        # Kill the app before next step
        if (-not $RunProc.HasExited) {
            Stop-Process -Id $RunProc.Id -Force -ErrorAction SilentlyContinue
        }
    } catch {
        Check 'launcher-first-run-alive' $false "Start-Process failed: $_"
    }
}

# ─────────────────────────────────────────────────────────────────────────────
W '── Check 3a: Bootstrap – Ollama direct download ──'

$OllamaSetupUrl  = 'https://ollama.com/download/OllamaSetup.exe'
$OllamaSetupTemp = "$env:TEMP\OllamaSetup.exe"
$OllamaInstalled = $false

try {
    W "Downloading OllamaSetup.exe from $OllamaSetupUrl ..."
    $wc = New-Object System.Net.WebClient
    $wc.DownloadFile($OllamaSetupUrl, $OllamaSetupTemp)

    $ollamaSize = (Get-Item $OllamaSetupTemp -ErrorAction SilentlyContinue).Length
    Check 'ollama-setup-downloaded' (($ollamaSize -gt 10MB)) `
        "Size: $([math]::Round($ollamaSize/1MB,2)) MB at $OllamaSetupTemp"

    W 'Running OllamaSetup.exe /VERYSILENT /SP- /SUPPRESSMSGBOXES ...'
    $oProc = Start-Process -FilePath $OllamaSetupTemp `
        -ArgumentList '/VERYSILENT', '/SP-', '/SUPPRESSMSGBOXES' `
        -PassThru -Wait
    Data 'ollama-installer-exit' "$($oProc.ExitCode)"

    # Allow installer to finish writing
    Start-Sleep -Seconds 5
    Remove-Item $OllamaSetupTemp -Force -ErrorAction SilentlyContinue

    $OllamaExe = "$env:LOCALAPPDATA\Programs\Ollama\ollama.exe"
    $OllamaInstalled = Test-Path $OllamaExe
    Check 'ollama-exe-exists' $OllamaInstalled "Path: $OllamaExe"

    if ($OllamaInstalled) {
        try {
            $verOut = & $OllamaExe --version 2>&1
            Data 'ollama-version-raw' ($verOut | Out-String).Trim()
            # Parse "ollama version is X.Y.Z"
            $verMatch = [regex]::Match(($verOut | Out-String), '(\d+\.\d+\.\d+)')
            if ($verMatch.Success) {
                $parts = $verMatch.Value.Split('.')
                $major = [int]$parts[0]; $minor = [int]$parts[1]; $patch = [int]$parts[2]
                # Minimum: 0.15.6
                $meetsMin = ($major -gt 0) -or ($minor -gt 15) -or ($minor -eq 15 -and $patch -ge 6)
                Check 'ollama-version-min-0.15.6' $meetsMin "Parsed: $($verMatch.Value); meets >=0.15.6: $meetsMin"
            } else {
                Check 'ollama-version-min-0.15.6' $false "Could not parse version from: $($verOut | Out-String)"
            }
        } catch {
            Check 'ollama-version-min-0.15.6' $false "ollama --version failed: $_"
        }
    }
} catch {
    Check 'ollama-setup-downloaded' $false "Download failed: $_"
}

# ─────────────────────────────────────────────────────────────────────────────
W '── Check 3b: Bootstrap – Claude Code install ──'

$ClaudeInstalled = $false
try {
    W 'Running: irm https://claude.ai/install.ps1 | iex ...'
    $claudeProc = Start-Process -FilePath 'powershell.exe' `
        -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', `
                      '-Command', 'irm https://claude.ai/install.ps1 | iex' `
        -Wait -PassThru
    Data 'claude-install-exit' "$($claudeProc.ExitCode)"

    $ClaudeExe = "$env:USERPROFILE\.local\bin\claude.exe"
    $ClaudeInstalled = Test-Path $ClaudeExe
    Check 'claude-exe-exists' $ClaudeInstalled "Path: $ClaudeExe"

    if ($ClaudeInstalled) {
        try {
            $cvOut = & $ClaudeExe --version 2>&1
            Data 'claude-version' ($cvOut | Out-String).Trim()
        } catch {
            Data 'claude-version' "Could not get version: $_"
        }
    }
} catch {
    Check 'claude-exe-exists' $false "Install failed: $_"
}

# ─────────────────────────────────────────────────────────────────────────────
W '── Check 4: Second run – post-install doctor check ──'

if (-not $LauncherExe) {
    Check 'launcher-second-run' $false 'launcher.exe not found — skipping'
} elseif (-not ($OllamaInstalled -or $ClaudeInstalled)) {
    Check 'launcher-second-run' $false 'Neither Ollama nor Claude installed — skipping second run'
} else {
    # Write an AppData settings.json to avoid wizard popup
    $AppDir = "$env:APPDATA\free-claude-code"
    New-Item -ItemType Directory -Force $AppDir | Out-Null
    $SettingsJson = @{
        hotkey          = 'Alt+H'
        model           = 'minimax-m2.7:cloud'
        cautious_mode   = $false
        background_mode = $true
        working_dir     = "$env:TEMP\fcc-sandbox-test"
        autostart       = $false
        history         = @()
        signin_state    = 'Unknown'
    } | ConvertTo-Json
    Set-Content -Path "$AppDir\settings.json" -Value $SettingsJson -Encoding UTF8
    Data 'settings-written' "$AppDir\settings.json"

    # Create working dir
    New-Item -ItemType Directory -Force "$env:TEMP\fcc-sandbox-test" | Out-Null

    $RunProc2 = $null
    try {
        $RunProc2 = Start-Process -FilePath $LauncherExe -ArgumentList '--run', 'test' -PassThru
        Start-Sleep -Seconds 10

        $StillAlive2 = -not $RunProc2.HasExited
        Check 'launcher-second-run-alive' $StillAlive2 `
            "PID $($RunProc2.Id); ExitCode=$(if ($RunProc2.HasExited) { $RunProc2.ExitCode } else { 'running' })"

        # Look for a run log to see what happened
        $LogsDir = "$AppDir\logs"
        $RunLog  = Get-ChildItem $LogsDir -Filter 'fcc-*.log' -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime | Select-Object -Last 1
        if ($RunLog) {
            Data 'run-log-path' $RunLog.FullName
            $LogContent = Get-Content $RunLog.FullName -Raw -ErrorAction SilentlyContinue
            $HasAuthError = $LogContent -match '(?i)auth|signin|401|403|unauthorized|token'
            Check 'run-log-has-auth-or-wizard-indicator' $HasAuthError `
                "Log tail (last 20 lines):`n$(Get-Content $RunLog.FullName -Tail 20 | Out-String)"
        } else {
            Data 'run-log-path' 'No log file appeared within 10s'
            Check 'run-log-has-auth-or-wizard-indicator' $false 'No run log found'
        }

        if ($RunProc2 -and -not $RunProc2.HasExited) {
            Stop-Process -Id $RunProc2.Id -Force -ErrorAction SilentlyContinue
        }
    } catch {
        Check 'launcher-second-run-alive' $false "Start-Process failed: $_"
    }
}

# ─────────────────────────────────────────────────────────────────────────────
W '── Summary ──'
$passCount = ($Lines | Where-Object { $_ -match '^\[PASS\]' }).Count
$failCount = ($Lines | Where-Object { $_ -match '^\[FAIL\]' }).Count
$dataCount = ($Lines | Where-Object { $_ -match '^\[DATA\]' }).Count
W "Total: $passCount PASS  /  $failCount FAIL  /  $dataCount DATA"
W "Report written to: $ReportPath"
W ''
W '=== END REPORT ==='

WriteReport
