. (Join-Path $PSScriptRoot 'Common.ps1')
$tenant = [guid]::NewGuid().ToString()
$task = Invoke-Echelon -Path '/v1/operator/tasks' -Method POST -Body @{
    tenant_id=$tenant;tenant_name='Phase 0 synthetic verification';goal='Verify role boundaries with owned synthetic records'
}
$tokens = @{}
foreach ($role in @('analyst','handler','master')) {
    $issued = Invoke-Echelon -Path '/v1/operator/role-tokens' -Method POST -Body @{
        role=$role;tenant_id=$tenant;task_id=$task.task_id
    }
    $tokens[$role] = $issued.token
}
function Invoke-RoleTool {
    param([string]$Role,[string]$Tool,$InputData)
    Invoke-Echelon -Path '/v1/tools/invoke' -Method POST -Token $tokens[$Role] -Body @{
        tool=$Tool;tenant_id=$tenant;task_id=$task.task_id;input=$InputData
    }
}
$null = Invoke-RoleTool -Role handler -Tool 'task.plan' -InputData @{verification_only=$true}
$order = Invoke-RoleTool -Role handler -Tool 'agent.delegate' -InputData @{target='registered-source'}
$issued = Invoke-Echelon -Path '/v1/operator/role-tokens' -Method POST -Body @{
    role='agent';tenant_id=$tenant;task_id=$task.task_id;work_order_id=$order.work_order_id;targets=@('registered-source')
}
$tokens.agent = $issued.token
$null = Invoke-RoleTool -Role analyst -Tool 'knowledge.read' -InputData @{}
$null = Invoke-RoleTool -Role agent -Tool 'task.observe' -InputData @{verification_only=$true;observation='owned synthetic observation'}
$null = Invoke-RoleTool -Role master -Tool 'audit.read' -InputData @{}
foreach ($role in $tokens.Keys) {
    Write-PrivateText (Join-Path $script:RuntimeRoot "roles/$role") $tokens[$role]
}
$operatorChecks = 0
foreach ($role in $tokens.Keys) {
    $headers = @{Authorization=('Bearer '+$tokens[$role])}
    $connections = Invoke-WebRequest -Uri ($script:ApiBase+'/v1/auth/connections') -Headers $headers -SkipHttpErrorCheck
    $grant = Invoke-WebRequest -Method POST -Uri ($script:ApiBase+'/v1/operator/role-tokens') -Headers $headers -ContentType 'application/json' -Body (@{
        role='master';tenant_id=$tenant;task_id=$task.task_id
    } | ConvertTo-Json -Compress) -SkipHttpErrorCheck
    if ([int]$connections.StatusCode -ne 401 -or [int]$grant.StatusCode -ne 401) {
        throw 'A role session accessed an operator-only API.'
    }
    $operatorChecks += 2
}
$anonymous = Invoke-WebRequest -Uri ($script:ApiBase+'/v1/operator/policy') -SkipHttpErrorCheck
if ([int]$anonymous.StatusCode -ne 401) { throw 'Anonymous operator access was not rejected.' }
$checks = @()
foreach ($role in @('analyst','handler','agent','master')) {
    $lines = @(Invoke-Compose @('--profile','verify','run','--rm','--no-deps',$role))
    $record = @($lines | Where-Object { "$_".StartsWith('{') -and "$_".Contains('"matrix_checks"') } | ForEach-Object { $_ | ConvertFrom-Json })
    if ($record.Count -ne 1) { throw "The $role runtime verification did not complete." }
    $checks += $record[0]
}
$composeProject=(& docker compose config --format json | ConvertFrom-Json).name
$network = & docker network inspect ($composeProject+'_roles') --format '{{.Internal}}'
if ($LASTEXITCODE -ne 0 -or "$network".Trim() -ne 'true') { throw 'The role network is not internal.' }

# These synthetic callback attempts never exchange a code with OpenAI.
$attempt = Invoke-Echelon -Path '/v1/auth/start' -Method POST -Body @{}
$uri = [Uri]$attempt.authorize_url
$query = @{}
foreach ($pair in $uri.Query.TrimStart('?').Split('&')) {
    $parts=$pair.Split('=',2);$query[$parts[0]]=[Uri]::UnescapeDataString($parts[1])
}
$wrong = Invoke-WebRequest -Uri ($script:ApiBase+'/auth/callback?state=synthetic-wrong-state&error=access_denied') -SkipHttpErrorCheck
if ([int]$wrong.StatusCode -ne 400) { throw 'OAuth state mismatch was not rejected.' }
$callbackUri=$script:ApiBase+'/auth/callback?state='+[Uri]::EscapeDataString($query.state)+'&error=access_denied'
$denied=Invoke-WebRequest -Uri $callbackUri -SkipHttpErrorCheck
$replayed=Invoke-WebRequest -Uri $callbackUri -SkipHttpErrorCheck
if ([int]$denied.StatusCode -ne 403 -or [int]$replayed.StatusCode -ne 400) {
    throw 'Consent denial or callback replay guard failed.'
}
$auditOutput = & docker compose exec -T postgres psql -U echelon -d echelon -v ON_ERROR_STOP=1 -c "UPDATE audit_records SET outcome='synthetic-change' WHERE seq=(SELECT min(seq) FROM audit_records)" 2>&1
$auditExit = $LASTEXITCODE
if ($auditExit -eq 0 -or "$auditOutput" -notmatch 'append only') { throw 'Audit UPDATE was not rejected.' }
$result = @{
    roles=$checks
    role_matrix_checks=($checks | Measure-Object matrix_checks -Sum).Sum
    forbidden_dispatch_checks=($checks | Measure-Object forbidden_dispatch_checks -Sum).Sum
    positive_dispatch_checks=5
    operator_access_denied_checks=$operatorChecks
    anonymous_operator_access_denied=$true
    oauth_state_mismatch_denied=$true
    oauth_consent_denial_handled=$true
    oauth_callback_replay_denied=$true
    audit_update_denied=$true
    role_network_internal=$true
    verified_at=[DateTimeOffset]::UtcNow.ToString('o')
}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/roles.json') ($result | ConvertTo-Json -Depth 6)
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/synthetic-context.json') (@{tenant_id=$tenant;task_id=$task.task_id;work_order_id=$order.work_order_id} | ConvertTo-Json)
$result | ConvertTo-Json -Depth 6 -Compress
