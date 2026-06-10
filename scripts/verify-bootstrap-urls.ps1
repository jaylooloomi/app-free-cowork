#Requires -Version 7
<#
.SYNOPSIS
    Validates the three live URLs the bootstrap subsystem depends on.

.DESCRIPTION
    1. https://ollama.com/download/OllamaSetup.exe
       - HEAD to confirm 200 + Content-Length > 100 MB
       - Downloads first 1 MB (Range: bytes=0-1048575) to check MZ magic bytes
    2. https://claude.ai/install.ps1
       - GET to confirm 200 + content contains "claude" + a param block
       - Prints first 5 lines
    3. https://ollama.com/api/tags
       - GET to confirm valid JSON with "models" array
       - Prints model count + checks for "minimax-m2.7" presence

    Exit code 0 = all checks passed, 1 = one or more failed.
#>

$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$OverallPass = $true
$Results = [System.Collections.Generic.List[PSCustomObject]]::new()

function Row([string]$check, [bool]$pass, [string]$detail) {
    $script:OverallPass = $script:OverallPass -and $pass
    $script:Results.Add([PSCustomObject]@{
        Check  = $check
        Status = if ($pass) { 'PASS' } else { 'FAIL' }
        Detail = $detail
    })
}

function PrintTable {
    Write-Host ''
    Write-Host ('─' * 90)
    Write-Host ('{0,-48} {1,-6} {2}' -f 'Check', 'Status', 'Detail')
    Write-Host ('─' * 90)
    foreach ($r in $Results) {
        $colour = if ($r.Status -eq 'PASS') { 'Green' } else { 'Red' }
        Write-Host ('{0,-48} ' -f $r.Check) -NoNewline
        Write-Host ('{0,-6} ' -f $r.Status) -ForegroundColor $colour -NoNewline
        Write-Host $r.Detail
    }
    Write-Host ('─' * 90)
    Write-Host ''
}

# ─────────────────────────────────────────────────────────────────────────────
Write-Host '=== verify-bootstrap-urls.ps1 ===' -ForegroundColor Cyan
Write-Host "Run at: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host ''

# ─── 1. OllamaSetup.exe ──────────────────────────────────────────────────────
Write-Host '── 1. https://ollama.com/download/OllamaSetup.exe ──'

$OllamaUrl = 'https://ollama.com/download/OllamaSetup.exe'
$OllamaFinalUrl = $OllamaUrl

try {
    # HEAD request — follow redirects manually to capture final URL
    $hwr = [System.Net.HttpWebRequest]::Create($OllamaUrl)
    $hwr.Method            = 'HEAD'
    $hwr.AllowAutoRedirect = $true
    $hwr.Timeout           = 30000
    $hwr.UserAgent         = 'verify-bootstrap-urls/1.0'
    $resp = $hwr.GetResponse()
    $statusCode = [int]$resp.StatusCode
    $contentLen = $resp.ContentLength
    $OllamaFinalUrl = $resp.ResponseUri
    $resp.Dispose()

    Row 'ollama-setup-head-200' ($statusCode -eq 200) "HTTP $statusCode  FinalURL: $OllamaFinalUrl"
    Row 'ollama-setup-content-length-gt-100MB' ($contentLen -gt 100MB) `
        "Content-Length: $([math]::Round($contentLen/1MB,1)) MB  (need >100 MB)"
    Write-Host "  HEAD $statusCode  Content-Length=$([math]::Round($contentLen/1MB,1)) MB"
    Write-Host "  Final URL: $OllamaFinalUrl"
} catch {
    Row 'ollama-setup-head-200' $false "HEAD failed: $_"
    Row 'ollama-setup-content-length-gt-100MB' $false 'HEAD failed — no Content-Length'
}

# Download first 1 MB to check MZ header
Write-Host '  Downloading first 1 MB (Range: bytes=0-1048575) ...'
try {
    $req = [System.Net.HttpWebRequest]::Create($OllamaUrl)
    $req.Method            = 'GET'
    $req.AllowAutoRedirect = $true
    $req.Timeout           = 30000
    $req.UserAgent         = 'verify-bootstrap-urls/1.0'
    $req.AddRange(0, 1048575)   # bytes=0-1048575
    $rsp = $req.GetResponse()

    $stream = $rsp.GetResponseStream()
    $buf    = New-Object byte[] 1048576
    $read   = 0
    while ($read -lt $buf.Length) {
        $n = $stream.Read($buf, $read, ($buf.Length - $read))
        if ($n -eq 0) { break }
        $read += $n
    }
    $stream.Dispose()
    $rsp.Dispose()

    $magic = '0x{0:X2}{1:X2}' -f $buf[0], $buf[1]   # should be 0x4D5A = 'MZ'
    $isMZ  = $buf[0] -eq 0x4D -and $buf[1] -eq 0x5A
    Row 'ollama-setup-mz-magic' $isMZ "First 2 bytes: $magic  (expect 0x4D5A = MZ PE header)"
    Write-Host "  Read $read bytes; magic bytes: $magic  isMZ=$isMZ"
} catch {
    if ($_ -match '416|Range') {
        Write-Host '  Range header not supported — server returned 416; retrying full stream abort after 1 MB ...'
        try {
            $req2 = [System.Net.HttpWebRequest]::Create($OllamaUrl)
            $req2.Method            = 'GET'
            $req2.AllowAutoRedirect = $true
            $req2.Timeout           = 30000
            $req2.UserAgent         = 'verify-bootstrap-urls/1.0'
            $rsp2  = $req2.GetResponse()
            $strm2 = $rsp2.GetResponseStream()
            $buf2  = New-Object byte[] 2
            $strm2.Read($buf2, 0, 2) | Out-Null
            $strm2.Dispose()
            $rsp2.Dispose()
            $magic2 = '0x{0:X2}{1:X2}' -f $buf2[0], $buf2[1]
            $isMZ2  = $buf2[0] -eq 0x4D -and $buf2[1] -eq 0x5A
            Row 'ollama-setup-mz-magic' $isMZ2 "First 2 bytes (full-stream abort): $magic2  isMZ=$isMZ2"
        } catch {
            Row 'ollama-setup-mz-magic' $false "Full-stream fallback also failed: $_"
        }
    } else {
        Row 'ollama-setup-mz-magic' $false "Range download failed: $_"
    }
}

Write-Host ''

# ─── 2. claude.ai/install.ps1 ────────────────────────────────────────────────
Write-Host '── 2. https://claude.ai/install.ps1 ──'

$ClaudeUrl = 'https://claude.ai/install.ps1'
try {
    $req3 = [System.Net.HttpWebRequest]::Create($ClaudeUrl)
    $req3.Method            = 'GET'
    $req3.AllowAutoRedirect = $true
    $req3.Timeout           = 30000
    $req3.UserAgent         = 'verify-bootstrap-urls/1.0'
    $rsp3   = $req3.GetResponse()
    $status3 = [int]$rsp3.StatusCode
    $reader  = New-Object System.IO.StreamReader($rsp3.GetResponseStream())
    $content = $reader.ReadToEnd()
    $reader.Dispose()
    $rsp3.Dispose()

    Row 'claude-install-ps1-200'     ($status3 -eq 200)                       "HTTP $status3  Length=$($content.Length) chars"
    Row 'claude-install-ps1-has-claude' ($content -match '(?i)claude')         "Content contains 'claude': $($content -match '(?i)claude')"
    $hasParam = $content -match '(?i)param\s*\('
    Row 'claude-install-ps1-has-param'  $hasParam "Content contains 'param(': $hasParam"

    Write-Host "  HTTP $status3  $($content.Length) chars"
    Write-Host '  First 5 lines:'
    $content -split "`r?`n" | Select-Object -First 5 | ForEach-Object { Write-Host "    $_" }
} catch {
    Row 'claude-install-ps1-200'        $false "GET failed: $_"
    Row 'claude-install-ps1-has-claude' $false 'GET failed'
    Row 'claude-install-ps1-has-param'  $false 'GET failed'
}

Write-Host ''

# ─── 3. ollama.com/api/tags ──────────────────────────────────────────────────
Write-Host '── 3. https://ollama.com/api/tags ──'

$TagsUrl = 'https://ollama.com/api/tags'
try {
    $req4   = [System.Net.HttpWebRequest]::Create($TagsUrl)
    $req4.Method            = 'GET'
    $req4.AllowAutoRedirect = $true
    $req4.Timeout           = 30000
    $req4.UserAgent         = 'verify-bootstrap-urls/1.0'
    $rsp4   = $req4.GetResponse()
    $status4 = [int]$rsp4.StatusCode
    $reader4 = New-Object System.IO.StreamReader($rsp4.GetResponseStream())
    $json4   = $reader4.ReadToEnd()
    $reader4.Dispose()
    $rsp4.Dispose()

    $obj4 = $json4 | ConvertFrom-Json -ErrorAction Stop
    $models = $obj4.models
    $modelCount = if ($models) { ($models | Measure-Object).Count } else { 0 }
    $hasModels  = $modelCount -gt 0
    $hasMinMax  = ($models | Where-Object { $_.name -like 'minimax-m2.7*' -or $_.model -like 'minimax-m2.7*' }).Count -gt 0

    Row 'ollama-api-tags-200'          ($status4 -eq 200)   "HTTP $status4"
    Row 'ollama-api-tags-valid-json'   $true                 "Valid JSON; models count: $modelCount"
    Row 'ollama-api-tags-has-minimax'  $hasMinMax            "minimax-m2.7 present: $hasMinMax"

    Write-Host "  HTTP $status4  Model count: $modelCount"
    Write-Host "  minimax-m2.7 present: $hasMinMax"
    if ($models) {
        Write-Host '  First 5 models:'
        $models | Select-Object -First 5 | ForEach-Object {
            $n = if ($_.name) { $_.name } elseif ($_.model) { $_.model } else { $_ | Out-String }
            Write-Host "    $n"
        }
    }
} catch [System.Net.WebException] {
    $status4x = if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { 0 }
    if ($status4x -eq 404) {
        # 404 is a known case if the endpoint doesn't exist publicly
        Row 'ollama-api-tags-200'        $false "HTTP 404 — endpoint may require auth or may be local-only"
        Row 'ollama-api-tags-valid-json' $false '404 — no body'
        Row 'ollama-api-tags-has-minimax' $false '404 — no body'
        Write-Host "  HTTP 404 returned — ollama.com/api/tags may be local-server only"
    } else {
        Row 'ollama-api-tags-200'        $false "WebException: $_ (HTTP $status4x)"
        Row 'ollama-api-tags-valid-json' $false 'Request failed'
        Row 'ollama-api-tags-has-minimax' $false 'Request failed'
    }
} catch {
    Row 'ollama-api-tags-200'        $false "GET failed: $_"
    Row 'ollama-api-tags-valid-json' $false 'GET failed'
    Row 'ollama-api-tags-has-minimax' $false 'GET failed'
}

# ─────────────────────────────────────────────────────────────────────────────
PrintTable

$passCount = ($Results | Where-Object Status -eq 'PASS').Count
$failCount = ($Results | Where-Object Status -eq 'FAIL').Count
$resultColor = if ($OverallPass) { 'Green' } else { 'Yellow' }
Write-Host "Result: $passCount/$($Results.Count) checks passed" -ForegroundColor $resultColor
Write-Host ''

exit $(if ($OverallPass) { 0 } else { 1 })
