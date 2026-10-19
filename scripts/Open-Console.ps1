param([switch]$NoBrowser)
. (Join-Path $PSScriptRoot 'Common.ps1')
$url = 'http://127.0.0.1:18431/'
if (-not $NoBrowser) { Start-Process $url }
@{ status='console_ready'; url=$url; login_required=$false } | ConvertTo-Json -Compress
