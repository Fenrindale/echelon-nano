# Real Worker/Broker regression, isolated from user Tasks and external services.
param([switch]$WithOAuth)
. (Join-Path $PSScriptRoot 'Common.ps1')
if ($script:ApiBase -ne 'http://127.0.0.1:18432') { throw 'Worker data verification requires the isolated port 18432 copy.' }
if ((docker compose config --format json | ConvertFrom-Json).name -ne 'echelon-nano-dragdrop-verification') { throw 'Wrong verification project.' }
$checks = 0
function Check-WorkerData([bool]$Value, [string]$Message) { if (-not $Value) { throw $Message }; $script:checks++ }
$tenant = (Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned large Worker data verification'}).id
$contract = @{kind='work';allowed_tools=@('artifact.write','shell.exec');input_artifacts=@();max_work_orders=3;output_format='Verified complete ledger and compact summary'}
$task = (Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned Worker data';goal='Owned structured source reconciliation';contract=$contract}).task_id
function Delegate-WorkerData($Sources) {
    Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task;tool='agent.delegate';input=@{request_id=[guid]::NewGuid().ToString();target='owned:worker-data';objective='Compute from exact granted source files';expected_output='Verified ledger';input_artifacts=@($Sources)}}
}
function Invoke-WorkerData([string]$Tool, $Payload) {
    Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$task;tool=$Tool;target='owned:worker-data';input=$Payload} $script:workerDataToken
}
function Set-WorkerDataToken([string]$Order) {
    $script:workerDataToken = (Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$task;work_order_id=$Order;targets=@('owned:worker-data')}).token
}
$first = Delegate-WorkerData @()
Set-WorkerDataToken $first.work_order_id
$sources = @()
$attachments = @()
foreach ($page in 0..1) {
    $rows = @((500*$page)..(500*$page+999) | ForEach-Object {
        $state = if ($_ % 3 -eq 0) { 'OPEN' } else { 'CLOSED' }
        if ($page -eq 1 -and $_ -eq 750) { $state = 'CLOSED' }
        @{uuid=('owned-'+$_);status=$state;payload=('Owned synthetic row evidence. '*8)}
    })
    $inner = @{incidents=$rows} | ConvertTo-Json -Depth 8 -Compress
    $body = @{content=@(@{type='text';text=$inner})} | ConvertTo-Json -Depth 8 -Compress
    Check-WorkerData ([Text.Encoding]::UTF8.GetByteCount($body) -gt 131072) 'Source does not cross the old input-file limit.'
    $source = Invoke-WorkerData 'artifact.write' @{request_id=[guid]::NewGuid().ToString();name='mcp-output.json';media_type='application/json';content=$body}
    $sources += $source.artifact.id
    $attachments += @{request_id=[guid]::NewGuid().ToString();name=("page-$page.json");media_type='application/json';content=$body}
}
$unselected = Invoke-WorkerData 'artifact.write' @{request_id=[guid]::NewGuid().ToString();name='not-selected.txt';content='OWNED_NOT_GRANTED'}
$null = Invoke-WorkerData 'work_order.complete' @{outcome='completed';summary='Two exact sources retained';evidence=@();artifacts=$sources;limitations=@()}
$second = Delegate-WorkerData $sources
Set-WorkerDataToken $second.work_order_id
$scriptText = @'
python3 - <<'PY'
import json, pathlib, hashlib
m=json.loads(pathlib.Path('echelon-inputs.json').read_text())
assert len(m['inputs']) == 2
assert len({v['path'] for v in m['inputs']}) == 2
assert {v['name'] for v in m['inputs']} == {'mcp-output.json'}
assert not pathlib.Path('mcp-output.json').exists()
assert not pathlib.Path('not-selected.txt').exists()
rows=[]
for f in m['inputs']:
    raw=pathlib.Path(f['path']).read_bytes()
    assert len(raw) == f['bytes']
    assert hashlib.sha256(raw).hexdigest() == f['sha256']
    rows += json.loads(json.loads(raw)['content'][0]['text'])['incidents']
versions={}
for row in rows:
    versions.setdefault(row['uuid'],set()).add(row['status'])
summary={'rows':len(rows),'unique':len(versions),'duplicates':len(rows)-len(versions),'conflicts':sum(len(v)>1 for v in versions.values())}
assert summary == {'rows':2000,'unique':1500,'duplicates':500,'conflicts':1},summary
pathlib.Path('summary.json').write_text(json.dumps(summary))
# Complete source-derived ledger, including duplicate/contradictory versions.
pathlib.Path('ledger.json').write_text(json.dumps({'rows':rows,'source_manifest':m,'summary':summary,'analysis_padding':'x'*1100000}))
print('S'*80000+'STDOUT_TAIL_PRESERVED',end='')
PY
'@
$key = [guid]::NewGuid().ToString()
$result = Invoke-WorkerData 'shell.exec' @{request_id=$key;script=$scriptText;outputs=@('summary.json','ledger.json');timeout_seconds=30}
Check-WorkerData ($result.exit_code -eq 0 -and -not $result.timed_out) 'Actual Worker computation failed.'
Check-WorkerData ($result.stdout.Length -gt 65536 -and $result.stdout.EndsWith('STDOUT_TAIL_PRESERVED')) 'Worker stdout was truncated.'
Check-WorkerData ($result.input_count -eq 2 -and $result.input_manifest -eq 'echelon-inputs.json') 'Input manifest metadata is missing.'
$ledger = @($result.artifacts | Where-Object name -eq 'ledger.json')[0]
Check-WorkerData ($ledger.bytes -gt 1048576) 'Complete output did not cross the former artifact/output quotas.'
$full = Invoke-Echelon "/v1/operator/artifacts/$($ledger.id)?tenant_id=$tenant"
$hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($full.content))).ToLowerInvariant()
Check-WorkerData ($hash -ceq $ledger.sha256) 'Complete ledger differs from its immutable hash.'
$data = $full.content | ConvertFrom-Json
Check-WorkerData ($data.rows.Count -eq 2000 -and $data.summary.unique -eq 1500 -and $data.summary.conflicts -eq 1) 'Original rows, duplicates or contradictions were lost.'
$replay = Invoke-WorkerData 'shell.exec' @{request_id=$key;script=$scriptText;outputs=@('summary.json','ledger.json');timeout_seconds=30}
Check-WorkerData ($replay.artifacts[1].id -eq $result.artifacts[1].id) 'Replay changed or duplicated completed output.'
$denied = Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization='Bearer '+$script:workerDataToken} -ContentType 'application/json' -Body (@{tenant_id=$tenant;task_id=$task;tool='artifact.read';target='owned:worker-data';input=@{artifact_id=$unselected.artifact.id}} | ConvertTo-Json -Depth 6 -Compress) -SkipHttpErrorCheck
Check-WorkerData ([int]$denied.StatusCode -eq 404) 'Unselected sibling artifact leaked.'
$bad = Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization='Bearer '+$script:workerDataToken} -ContentType 'application/json' -Body (@{tenant_id=$tenant;task_id=$task;tool='shell.exec';target='owned:worker-data';input=@{request_id=[guid]::NewGuid().ToString();script='true';outputs=@('../escape')}} | ConvertTo-Json -Depth 6 -Compress) -SkipHttpErrorCheck
Check-WorkerData ([int]$bad.StatusCode -eq 400 -and ($bad.Content|ConvertFrom-Json).error.code -eq 'worker_output_path_invalid') 'Worker validation did not retain its precise cause.'
$null = Invoke-WorkerData 'work_order.complete' @{outcome='completed';summary='Actual code reconciled all rows and preserved source/output hashes';evidence=@();artifacts=@($result.artifacts.id);limitations=@()}
$evidence = @{passed=$true;assertions=$checks;task_id=$task;source_count=2;rows=2000;unique=1500;conflicts=1;output_bytes=$ledger.bytes;output_sha256=$hash;stdout_bytes=$result.stdout.Length;isolation_preserved=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/worker-data.json') ($evidence|ConvertTo-Json -Depth 6)
$evidence | ConvertTo-Json -Compress
if ($WithOAuth) {
    $goal = 'Reconcile the two attached MCP incident pages using their actual nested JSON bodies. Produce worker-data-result.json with exactly the numeric fields rows (all returned rows), unique (distinct uuid), duplicates (rows minus unique), and conflicts (UUIDs with more than one distinct status). Preserve conflicting source versions. Use local Python through shell.exec and the echelon-inputs.json manifest; verify input hashes and the actual result. Do not manually transcribe/count rows through the model. All business inputs are supplied. No network, credentials, MCP installation or central knowledge creation is needed. Disable retention and audit. Handler verifies the real calculation and final artifact against the original goal, then finishes.'
    $launch = Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal=$goal;attachments=$attachments}
    $run = $launch.run_id
    Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/worker-data-loop-active.json') (@{run=$run;task=$launch.task_id;tenant=$tenant;goal=$goal}|ConvertTo-Json -Depth 5)
    $deadline = [DateTimeOffset]::UtcNow.AddMinutes(15)
    do {
        $state = Invoke-Echelon "/v1/operator/runs/$run"
        if ($state.status -notin @('queued','running')) { break }
        if ([DateTimeOffset]::UtcNow -gt $deadline) { throw 'Worker data LLM workflow timed out; preserve and inspect its checkpoint.' }
        Start-Sleep -Seconds 2
    } while ($true)
    Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/worker-data-loop-run.json') ($state|ConvertTo-Json -Depth 60)
    Check-WorkerData ($state.status -eq 'completed' -and $state.result.outcome -eq 'completed') "Worker data LLM workflow failed: $($state.status),$($state.error_code)"
    $t = Invoke-Echelon "/v1/operator/tasks/$($launch.task_id)?tenant_id=$tenant"
    $final = @($t.artifacts|Where-Object name -eq 'worker-data-result.json')[-1]
    $actual = Invoke-Echelon "/v1/operator/artifacts/$($final.id)?tenant_id=$tenant"
    $answer = $actual.content|ConvertFrom-Json
    Check-WorkerData ($answer.rows -eq 2000 -and $answer.unique -eq 1500 -and $answer.duplicates -eq 500 -and $answer.conflicts -eq 1) 'LLM workflow returned incorrect source-derived counts.'
    $sql = "SELECT count(*) FROM execution_actions WHERE run_id='$run' AND request->>'tool'='shell.exec' AND outcome->>'ok'='true' AND outcome->'result'->>'exit_code'='0';"
    $executed = $sql | docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon
    Check-WorkerData ($LASTEXITCODE -eq 0 -and [int]("$executed".Trim()) -ge 1) 'Agent never executed actual successful local computation.'
    Check-WorkerData ($t.goal -ceq $goal) 'Original workflow goal changed.'
    $live = @{passed=$true;assertions=$checks;run_id=$run;task_id=$launch.task_id;calls=$state.calls;total_tokens=$state.total_tokens;actual_computation=$true;goal_preserved=$true;result_sha256=$final.sha256;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
    Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/worker-data-loop.json') ($live|ConvertTo-Json -Depth 6)
    $live|ConvertTo-Json -Compress
}
