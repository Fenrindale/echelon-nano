param([string]$ConnectionId, [string]$Model)
. (Join-Path $PSScriptRoot 'Common.ps1')
$connections = @( (Invoke-Echelon -Path '/v1/auth/connections').connections )
if (-not $ConnectionId) {
    if ($connections.Count -ne 1) { throw 'Choose one connected account with -ConnectionId.' }
    $ConnectionId = $connections[0].id
}
$connection = @($connections | Where-Object id -eq $ConnectionId)
if ($connection.Count -ne 1 -or -not $connection[0].plan_enabled) { throw 'ChatGPT plan usage has not been granted for this connection.' }
$models = @( (Invoke-Echelon -Path "/v1/auth/connections/$ConnectionId/models").models )
if (-not $Model) {
    if ($models.Count -eq 0) { throw 'No model is available to this account.' }
    $Model = $models[0].slug
}
if (-not ($models | Where-Object slug -eq $Model)) { throw 'The selected model is absent from the current account catalog.' }
$before = Invoke-Echelon -Path "/v1/auth/connections/$ConnectionId/probe" -Method POST -Body @{model=$Model}
if (-not $before.completed -or -not $before.matched) { throw 'The first inference probe failed.' }
$generation = [long]$before.generation
$operatorToken = Get-OperatorToken
$endpoint = $script:ApiBase + "/v1/auth/connections/$ConnectionId/refresh"
$refreshBody = @{expected_generation=$generation} | ConvertTo-Json -Compress
$refreshes = @(1..4 | ForEach-Object -Parallel {
    Invoke-RestMethod -Method POST -Uri $using:endpoint -Headers @{Authorization=('Bearer '+$using:operatorToken)} -ContentType 'application/json' -Body $using:refreshBody -TimeoutSec 60
} -ThrottleLimit 4)
$operatorToken = $null
if ($refreshes.Count -ne 4 -or @($refreshes | Where-Object generation -ne ($generation+1)).Count -ne 0) {
    throw 'Concurrent refreshes did not coalesce to one new credential generation.'
}
$after = Invoke-Echelon -Path "/v1/auth/connections/$ConnectionId/probe" -Method POST -Body @{model=$Model}
if (-not $after.completed -or -not $after.matched -or $after.generation -ne ($generation+1)) {
    throw 'Inference with the renewed credential failed.'
}
$result = @{
    login_verified=$true
    model=$Model
    inference_before_refresh=$true
    refresh_rotation_verified=$true
    concurrent_refresh_callers=4
    credential_generations_advanced=1
    inference_after_refresh=$true
    verified_at=[DateTimeOffset]::UtcNow.ToString('o')
}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/live-oauth.json') ($result | ConvertTo-Json -Depth 4)
$result | ConvertTo-Json -Compress
