# Free Claude Code 一行安裝:irm <raw-url>/install.ps1 | iex
$ErrorActionPreference = "Stop"
$repo = "jaylooloomi/free-claude-code"
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
Write-Host "[OK] 安裝完成!按 Alt+H 開始使用。"
