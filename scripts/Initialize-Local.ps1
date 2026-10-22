param([switch]$SkipBuild)
. (Join-Path $PSScriptRoot 'Common.ps1')
Initialize-PrivateDirectory $script:RuntimeRoot
$bootstrap = Join-Path $script:RuntimeRoot 'bootstrap'
Initialize-PrivateDirectory $bootstrap
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'roles')
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$passwordPath = Join-Path $bootstrap 'postgres-password'
$workerPath = Join-Path $bootstrap 'worker-token'
if (-not (Test-Path -LiteralPath $workerPath)) { Write-PrivateText $workerPath (New-RuntimeSecret) }
if (-not (Test-Path -LiteralPath $passwordPath)) { Write-PrivateText $passwordPath (New-RuntimeSecret) }
$operatorPath = Join-Path $script:RuntimeRoot 'operator.dpapi'
if (Test-Path -LiteralPath $operatorPath) { $operator = Get-OperatorToken }
else {
    $operator = New-RuntimeSecret
    Protect-RuntimeBytes ([Text.Encoding]::UTF8.GetBytes($operator)) $operatorPath
}
$operatorInput = Join-Path $bootstrap 'operator-input'
$recoveryPlain = Join-Path $bootstrap 'vault-recovery.json'
$recoveryProtected = Join-Path $script:RuntimeRoot 'vault-recovery.dpapi'
Write-PrivateText $operatorInput $operator
if (Test-Path -LiteralPath $recoveryProtected) {
    [IO.File]::WriteAllBytes($recoveryPlain, (Unprotect-RuntimeBytes $recoveryProtected))
}
try {
    if (-not $SkipBuild) {
        # Direct builds also support Windows workspaces whose path contains non-ASCII characters.
        & docker build --target runtime -t echelon-nano:local $script:ProjectRoot
        if($LASTEXITCODE -ne 0){throw 'Core runtime build failed.'}
        & docker build --target worker -t echelon-nano-worker:local $script:ProjectRoot
        if($LASTEXITCODE -ne 0){throw 'Worker runtime build failed.'}
        & docker build --target mcp-host -t echelon-nano-mcp:local $script:ProjectRoot
        if($LASTEXITCODE -ne 0){throw 'MCP runtime build failed.'}
    }
    Invoke-Compose @('up','-d','postgres','vault')
    Start-Sleep -Seconds 2
    Invoke-Compose @('run','--rm','bootstrap')
} finally {
    if (Test-Path -LiteralPath $recoveryPlain) {
        Protect-RuntimeBytes ([IO.File]::ReadAllBytes($recoveryPlain)) $recoveryProtected
        Remove-Item -LiteralPath $recoveryPlain
    }
    if (Test-Path -LiteralPath $operatorInput) { Remove-Item -LiteralPath $operatorInput }
    $operator = $null
}
Invoke-Compose @('up','-d','--force-recreate','core','worker')
$ready = $false
for ($i=0; $i -lt 30; $i++) {
    try {
        $result = Invoke-RestMethod -Uri ($script:ApiBase + '/readyz') -TimeoutSec 2
        if ($result.status -eq 'ready') { $ready = $true; break }
    } catch { Start-Sleep -Milliseconds 500 }
}
if (-not $ready) { throw 'The Echelon core readiness check failed.' }
@{status='ready';url=$script:ApiBase} | ConvertTo-Json -Compress
