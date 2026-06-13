# FreeCowork 一行安裝:irm <raw-url>/install.ps1 | iex
$ErrorActionPreference = "Stop"
$repo = "jaylooloomi/FreeCowork"
$api = "https://api.github.com/repos/$repo/releases/latest"
Write-Host "[*] 取得最新版本資訊..."
try {
    $release = Invoke-RestMethod -Uri $api -UseBasicParsing
} catch {
    Write-Host "尚未有正式發行版,請至 https://github.com/$repo/releases 查看" -ForegroundColor Yellow
    exit 1
}
$asset = $release.assets | Where-Object { $_.name -like "*-setup.exe" } | Select-Object -First 1
if (-not $asset) { Write-Error "找不到安裝檔,請至 https://github.com/$repo/releases 手動下載"; exit 1 }
$out = Join-Path $env:TEMP $asset.name
Write-Host "[*] 下載 $($asset.name)..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $out -UseBasicParsing
Write-Host "[*] 安裝中(免系統管理員)..."
Start-Process -FilePath $out -ArgumentList "/S" -Wait
# NSIS currentUser 模式安裝至 $LOCALAPPDATA\<productName>(主程式 launcher.exe)
$exe = Join-Path $env:LOCALAPPDATA "Free Claude Code\launcher.exe"
if (Test-Path $exe) { Start-Process $exe; Write-Host "[OK] 安裝完成!按 Alt+H 開始使用。" }
else { Write-Host "[OK] 安裝完成!請從開始功能表啟動 Free Claude Code,之後按 Alt+H 使用。" }
