param([string]$ConnectionId, [string]$RetryAttemptId, [switch]$NoBrowser)
. (Join-Path $PSScriptRoot 'Common.ps1')
$body = @{}
if ($ConnectionId) { $body.connection_id = $ConnectionId }
if ($RetryAttemptId) { $body.retry_attempt_id = $RetryAttemptId }
$attempt = Invoke-Echelon -Path '/v1/auth/start' -Method POST -Body $body
$path = Join-Path $script:RuntimeRoot 'login-attempt.json'
Write-PrivateText $path (@{attempt_id=$attempt.attempt_id;started_at=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()} | ConvertTo-Json -Compress)
if (-not $NoBrowser) {
    # Visible browser interaction is required for the user's account choice and consent.
    Start-Process -FilePath $attempt.authorize_url
}
@{status='awaiting_user_login';attempt_id=$attempt.attempt_id;expires_in=$attempt.expires_in} | ConvertTo-Json -Compress
