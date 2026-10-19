. (Join-Path $PSScriptRoot 'Common.ps1')
$script:BetaAssertions=0
function Assert-Beta([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:BetaAssertions++}
function Domain-Beta([string]$Task,[string]$Tool,$InputData){Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$Task;tool=$Tool;input=$InputData}}
function Agent-Beta([string]$Tool,$InputData,[string]$Target='owned:beta') {Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$task.task_id;tool=$Tool;target=$Target;input=$InputData} $agent}
function Deny-Beta([string]$Tool,$InputData,[int]$Status=403,[string]$Target='owned:beta'){
 $r=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization='Bearer '+$agent} -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes((@{tenant_id=$tenant;task_id=$task.task_id;tool=$Tool;target=$Target;input=$InputData}|ConvertTo-Json -Depth 12 -Compress))) -SkipHttpErrorCheck
 Assert-Beta ([int]$r.StatusCode -eq $Status) "Expected $Status for $Tool"
}
Invoke-Compose @('--profile','verify-api','up','-d','--no-build','fixture')
$tenant=[guid]::NewGuid().ToString()
$room=Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned beta boundaries and console verification'}
$tenant=$room.id
$admin=(Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id
Assert-Beta ((Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id -eq $admin) 'Administration workspace was duplicated.'
$otherTenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned foreign beta boundary'}).id
$urls=@(@{url='http://fixture:8081/echo';allow_private=$true;allow_http=$true},@{url='http://fixture:8081/redirect';allow_private=$true;allow_http=$true},@{url='http://vault:8200/v1/sys/health';allow_private=$true;allow_http=$true})
$secretValue='Bearer '+(New-RuntimeSecret)
$secret=Invoke-Echelon '/v1/operator/secrets' 'POST' @{tenant_id=$tenant;name='Owned beta synthetic authentication';purpose='Verify broker injection, scope, rotation and redaction';allowed_urls=@('http://fixture:8081/echo');value=$secretValue}
$contract=@{kind='work';allowed_tools=@('shell.exec','external.fetch','artifact.write');urls=$urls;secret_refs=@($secret.secret_ref);input_artifacts=@();max_work_orders=2;output_format='Verified text artifacts'}
if(-not $secret.secret_ref){throw 'Secret reference metadata is missing.'}
$task=Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned beta boundary';goal='Owned beta tool and scope checks';contract=$contract}
$input=Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$task.task_id;artifact=@{request_id=[guid]::NewGuid().ToString();name='input.txt';content='Owned UTF-8 input'}}
$contract.input_artifacts=@($input.artifact.id)
$null=Invoke-Echelon "/v1/operator/tasks/$($task.task_id)" 'PATCH' @{tenant_id=$tenant;contract=$contract;reason='Grant only the owned input artifact'}
$foreign=Invoke-WebRequest -Uri ($script:ApiBase+"/v1/operator/artifacts/$($input.artifact.id)?tenant_id=$otherTenant") -Headers @{Authorization='Bearer '+(Get-OperatorToken)} -SkipHttpErrorCheck
Assert-Beta ([int]$foreign.StatusCode -eq 404) 'An artifact leaked across tenants.'
$delegation=@{request_id=[guid]::NewGuid().ToString();target='owned:beta';objective='Verify owned sandbox and read-only API';expected_output='Verified text artifacts'}
$order=Domain-Beta $task.task_id 'agent.delegate' $delegation
Assert-Beta ((Domain-Beta $task.task_id 'agent.delegate' $delegation).work_order_id -eq $order.work_order_id) 'Work Order replay duplicated work.'
$agent=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$task.task_id;work_order_id=$order.work_order_id;targets=@('owned:beta')}).token
$fetchKey=[guid]::NewGuid().ToString()
$fetched=Agent-Beta 'external.fetch' @{request_id=$fetchKey;url='http://fixture:8081/echo';secret_ref=$secret.secret_ref}
$echo=$fetched.content|ConvertFrom-Json
Assert-Beta ($fetched.status -eq 200 -and $echo.request_count -ge 1) 'The actual fixture was not fetched.'
Assert-Beta ((Agent-Beta 'artifact.read' @{artifact_id=$input.artifact.id}).content -eq 'Owned UTF-8 input') 'An explicitly approved input was not readable with the scoped artifact.read capability.'
Assert-Beta (-not $fetched.content.Contains($secretValue) -and -not $fetched.content.Contains([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($secretValue)))) 'A credential echo reached model-visible content.'
Assert-Beta ($echo.echo -eq '[REDACTED]' -and $echo.access_token -eq '[REDACTED]') 'Secret fields were not masked.'
Assert-Beta ((Agent-Beta 'external.fetch' @{request_id=$fetchKey;url='http://fixture:8081/echo';secret_ref=$secret.secret_ref}).content -eq $fetched.content) 'GET receipt replay changed the result.'
Deny-Beta 'external.fetch' @{request_id=[guid]::NewGuid().ToString();url='https://example.com'}
Deny-Beta 'external.fetch' @{request_id=[guid]::NewGuid().ToString();url='http://vault:8200/v1/sys/health'}
$redirect=Agent-Beta 'external.fetch' @{request_id=[guid]::NewGuid().ToString();url='http://fixture:8081/redirect'}
Assert-Beta ($redirect.status -eq 302) 'Redirect was followed.'
$shellScript=@'
test "$(id -u)" = 10002
test ! -r /root/.broker/token
test ! -r /run/secrets/vault-core-token
test "$(awk '/CapEff/ {print $2}' /proc/self/status)" = 0000000000000000
test "$(awk '/CapPrm/ {print $2}' /proc/self/status)" = 0000000000000000
test "$(cat input.txt)" = 'Owned UTF-8 input'
python3 - <<'PY'
import socket, pathlib
try:
    socket.socket(socket.AF_INET,socket.SOCK_STREAM)
except PermissionError:
    pass
else:
    raise AssertionError('sandbox child can create network sockets')
try:
    socket.create_connection(('1.1.1.1',443),timeout=1)
except OSError:
    pass
else:
    raise AssertionError('unexpected external network access')
try:
    pathlib.Path('/etc/echelon-owned-probe').write_text('denied')
except OSError:
    pass
else:
    raise AssertionError('unexpected root filesystem write')
pathlib.Path('result.txt').write_text('SANDBOX_BOUNDARIES_PASS',encoding='utf-8')
PY
'@
$shellKey=[guid]::NewGuid().ToString()
$sandbox=Agent-Beta 'shell.exec' @{request_id=$shellKey;script=$shellScript;outputs=@('result.txt');timeout_seconds=10}
Assert-Beta ($sandbox.exit_code -eq 0 -and -not $sandbox.timed_out) 'Sandbox UID, credentials, network, rootfs or input checks failed.'
Assert-Beta (@($sandbox.artifacts).Count -eq 1) 'Sandbox output was not recovered.'
Assert-Beta ((Agent-Beta 'shell.exec' @{request_id=$shellKey;script=$shellScript;outputs=@('result.txt');timeout_seconds=10}).artifacts[0].id -eq $sandbox.artifacts[0].id) 'Shell replay duplicated artifacts.'
Deny-Beta 'shell.exec' @{request_id=[guid]::NewGuid().ToString();script='ln -s /etc/passwd bad.txt';outputs=@('bad.txt')} 400
Deny-Beta 'shell.exec' @{request_id=[guid]::NewGuid().ToString();script='true';outputs=@('../escape.txt')} 400
$timed=Agent-Beta 'shell.exec' @{request_id=[guid]::NewGuid().ToString();script='sleep 4';outputs=@();timeout_seconds=1}
Assert-Beta ($timed.timed_out -and $null -eq $timed.exit_code) 'Worker timeout was not enforced.'
$rotated=Invoke-Echelon '/v1/operator/secrets' 'POST' @{tenant_id=$tenant;id=$secret.secret_ref;expected_version=$secret.version;name='Owned beta synthetic authentication';purpose='Rotated owned fixture';allowed_urls=@('http://fixture:8081/echo');value=('Bearer '+(New-RuntimeSecret))}
Assert-Beta ($rotated.version -gt $secret.version) 'Vault CAS rotation did not advance.'
$null=Invoke-Echelon "/v1/operator/secrets/$($secret.secret_ref)/disable" 'POST' @{tenant_id=$tenant}
Deny-Beta 'external.fetch' @{request_id=[guid]::NewGuid().ToString();url='http://fixture:8081/echo';secret_ref=$secret.secret_ref}
$null=Agent-Beta 'work_order.complete' @{outcome='completed';summary='Actual sandbox and broker checks passed';evidence=@();limitations=@();artifacts=@($sandbox.artifacts[0].id)}
Deny-Beta 'artifact.write' @{request_id=[guid]::NewGuid().ToString();name='late.txt';content='denied'}
$workerDisconnect=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'fixtures/verify-worker-recovery.py') -Raw | docker compose exec -T worker python3 -
if($LASTEXITCODE -ne 0){throw 'Native Worker disconnect recovery regression failed.'}
Assert-Beta (($workerDisconnect|ConvertFrom-Json).passed) 'Worker did not preserve its result after an RPC disconnect.'
# The local console opens without a login or pairing code. Its automatic
# session still protects browser mutations with same-origin/CSRF checks.
$pairResponse=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/ui/session') -Method POST -ContentType 'application/json' -Body '{}' -SessionVariable betaBrowser
$csrf=($pairResponse.Content|ConvertFrom-Json).csrf
Assert-Beta ("$($pairResponse.Headers['Set-Cookie'])" -match 'HttpOnly' -and "$($pairResponse.Headers['Set-Cookie'])" -match 'SameSite=Strict') 'The UI cookie is missing protections.'
$replay=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/ui/session') -Method POST -ContentType 'application/json' -Body '{}' -WebSession $betaBrowser
Assert-Beta (($replay.Content|ConvertFrom-Json).csrf -eq $csrf) 'Automatic console initialization replaced a valid session.'
$foreignSession=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/ui/session') -Method POST -Headers @{Origin='https://example.com'} -ContentType 'application/json' -Body '{}' -SkipHttpErrorCheck
Assert-Beta ([int]$foreignSession.StatusCode -eq 403) 'A foreign origin opened an automatic operator session.'
$deniedCsrf=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/operator/tenants') -Method POST -WebSession $betaBrowser -ContentType 'application/json' -Body '{"name":"must not be created"}' -SkipHttpErrorCheck
Assert-Beta ([int]$deniedCsrf.StatusCode -eq 403) 'Cookie mutation bypassed CSRF.'
$deniedOrigin=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/operator/tenants') -WebSession $betaBrowser -Headers @{Origin='https://example.com'} -SkipHttpErrorCheck
Assert-Beta ([int]$deniedOrigin.StatusCode -eq 403) 'A foreign Origin was accepted.'
$session=Invoke-RestMethod -Uri ($script:ApiBase+'/v1/ui/session') -WebSession $betaBrowser -Headers @{Origin=$script:ApiBase}
Assert-Beta ($session.csrf -eq $csrf) 'Browser session did not survive a subsequent request.'
$null=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/ui/logout') -Method POST -WebSession $betaBrowser -Headers @{'X-Echelon-Csrf'=$csrf} -ContentType 'application/json' -Body '{}'
$loggedOut=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/operator/tenants') -WebSession $betaBrowser -SkipHttpErrorCheck
Assert-Beta ([int]$loggedOut.StatusCode -eq 401) 'A logged-out session remained valid.'
$secretValue=$null;$csrf=$null;$agent=$null
$result=@{passed=$true;assertions=$script:BetaAssertions;tenant_id=$tenant;task_id=$task.task_id;work_order_id=$order.work_order_id;worker_uid=10002;external_network_blocked=$true;secret_echo_redacted=$true;tool_replay_verified=$true;ui_cookie_csrf_verified=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/beta-boundaries.json') ($result|ConvertTo-Json -Depth 8)
$result|ConvertTo-Json -Compress
