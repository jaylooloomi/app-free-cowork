# Windows Sandbox Fresh-Machine E2E Status

## Status: Artifacts ready — sandbox run deferred

Windows Sandbox is not enabled on the dev machine.
Enabling it requires: **Control Panel → Turn Windows features on/off → Windows Sandbox → OK → reboot** (admin rights required).
The sandbox run is deferred until that is done.

---

## Files created

| File | Purpose |
|---|---|
| `scripts/e2e-sandbox.wsb` | Double-click to launch the sandbox with correct folder mappings |
| `scripts/e2e-sandbox-inner.ps1` | Runs inside the sandbox; performs all checks; writes `C:\out\report.txt` |
| `scripts/sandbox-out/.gitkeep` | Keeps the output folder under version control so the WSB mapping resolves |
| `scripts/verify-bootstrap-urls.ps1` | Run on any machine; validates live URLs today without a sandbox |

---

## How to run the full E2E test

1. Build the NSIS installer (if not already done):
   ```
   cd launcher && npm run tauri build
   ```
   This produces `launcher\src-tauri\target\release\bundle\nsis\Free Claude Code_*_x64-setup.exe`.

2. Enable Windows Sandbox (admin + reboot required):
   - Open PowerShell as administrator:
     ```powershell
     Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All
     ```
   - Restart the machine.

3. From the repo root, double-click `scripts\e2e-sandbox.wsb` (or open it via the WSB file association).

4. The sandbox will open, the inner script will run automatically, and the structured report will be written to `scripts\sandbox-out\report.txt` on the host machine (the folder is mapped writable).

5. Read the report:
   ```
   cat scripts\sandbox-out\report.txt
   ```

---

## What each check in the sandbox proves

| Check | What it proves |
|---|---|
| `installer-found` | NSIS bundle exists and was mapped into the sandbox correctly |
| `install-dir-exists` | NSIS currentUser install writes to `%LOCALAPPDATA%\Free Claude Code` |
| `launcher-exe-exists` | The installed payload includes `launcher.exe` |
| `launcher-first-run-alive` | `launcher.exe --run test` does not crash immediately on a fresh machine (no dependencies); the app stays alive (i.e., routes to wizard, not a hard exit) |
| `no-crash-event-log` | No Windows Error Reporting crash event (EventID 1000) for launcher.exe |
| `ollama-setup-downloaded` | `https://ollama.com/download/OllamaSetup.exe` is reachable from the sandbox and >10 MB |
| `ollama-exe-exists` | The Inno Setup silent installer (`/VERYSILENT /SP- /SUPPRESSMSGBOXES`) works and places `ollama.exe` at `%LOCALAPPDATA%\Programs\Ollama\ollama.exe` — the exact path `bootstrap.rs` uses |
| `ollama-version-min-0.15.6` | Installed Ollama meets the minimum version gate in `doctor.rs` / `version.rs` |
| `claude-exe-exists` | `irm https://claude.ai/install.ps1 \| iex` works and places `claude.exe` at `%USERPROFILE%\.local\bin\claude.exe` — the primary path in `doctor.rs::default_claude_paths()` |
| `launcher-second-run-alive` | After both dependencies are installed, `launcher.exe --run test` still stays alive (does not panic); the run log records the actual doctor/auth outcome |
| `run-log-has-auth-or-wizard-indicator` | The app's log reflects that it reached the sign-in or wizard stage — proving the full bootstrap + doctor path executed |

---

## Live URL verification — results from 2026-06-11

Run: `pwsh -File scripts\verify-bootstrap-urls.ps1`

All **9/9 checks passed**.

| Check | Status | Key detail |
|---|---|---|
| `ollama-setup-head-200` | PASS | HTTP 200; served via GitHub release CDN |
| `ollama-setup-content-length-gt-100MB` | PASS | **1329.7 MB** (the full Ollama bundle) |
| `ollama-setup-mz-magic` | PASS | First 2 bytes `0x4D5A` — valid Windows PE binary |
| `claude-install-ps1-200` | PASS | HTTP 200; 3189 chars |
| `claude-install-ps1-has-claude` | PASS | Content references `claude` |
| `claude-install-ps1-has-param` | PASS | Content has a `param(` block — it is a valid PS script |
| `ollama-api-tags-200` | PASS | HTTP 200 |
| `ollama-api-tags-valid-json` | PASS | **41 cloud models** returned |
| `ollama-api-tags-has-minimax` | PASS | `minimax-m2.7` present in the catalog |

First 5 lines of `claude.ai/install.ps1`:
```powershell
param(
    [Parameter(Position=0)]
    [ValidatePattern('^(stable|latest|\d+\.\d+\.\d+(-[^\s]+)?)$')]
    [string]$Target = "latest"
)
```

---

## Notes and caveats

- The WSB file uses **relative paths** (`HostFolder`) per the WSB spec — paths are relative to the WSB file's own directory (the repo root). If you move `e2e-sandbox.wsb` out of `scripts/`, update the paths.
- The inner script handles the case where Ollama uses **Inno Setup** (`/VERYSILENT`), not NSIS, which is correct per `bootstrap.rs` line 106.
- The sandbox has internet access by default; if your policy disables it, checks 3a/3b/4 will FAIL with download errors — that is expected DATA, not a bug.
- `winget` availability in the sandbox is recorded as DATA (not required). The bootstrap fallback path (direct download) is what the inner script tests.
- The second `--run test` uses `signin_state = 'Unknown'` in settings, which means the launcher will attempt a cloud call and receive an auth error — this is intentional: we are verifying that the plumbing reaches that stage, not that signin succeeds in a headless environment.
