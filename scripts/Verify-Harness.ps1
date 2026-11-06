param([switch]$WithOAuth,[switch]$OAuthOnly)
. (Join-Path $PSScriptRoot 'Common.ps1')
. (Join-Path $PSScriptRoot 'KnowledgeFixture.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Harness verification requires the isolated port 18432 copy.'}
$checks=0
function Check-Harness([bool]$Ok,[string]$Message){if(-not $Ok){throw $Message};$script:checks++}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned harness verification'}).id
$task=$null
if(-not $OAuthOnly){
$contract=@{kind='work';allowed_tools=@('shell.exec','artifact.write');max_work_orders=0;output_format='Owned text outputs'}
$task=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned harness verification';goal='Verify owned asynchronous file work and scope';contract=$contract}).task_id
$handler=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='handler';tenant_id=$tenant;task_id=$task}).token
function Handler-Harness([string]$Tool,$InputData){Invoke-Echelon '/v1/tools/invoke' 'POST' @{tool=$Tool;tenant_id=$tenant;task_id=$task;input=$InputData} $handler}
$orders=@(1..12|ForEach-Object{(Handler-Harness 'agent.delegate' @{request_id=[guid]::NewGuid().ToString();target='owned:harness';objective="Owned attempt $_";expected_output='Owned files'}).work_order_id})
Check-Harness ($orders.Count -eq 12) 'The historical eight-Work-Order cap remains.'
$order=$orders[0]
$agent=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$task;work_order_id=$order;targets=@('owned:harness')}).token
function Agent-Harness([string]$Tool,$InputData,[string]$Token=$agent){Invoke-Echelon '/v1/tools/invoke' 'POST' @{tool=$Tool;tenant_id=$tenant;task_id=$task;target='owned:harness';input=$InputData} $Token}
function Deny-Harness([string]$Tool,$InputData,[string]$Token,[int]$Status){
 $r=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization=('Bearer '+$Token)} -ContentType 'application/json' -Body (@{tool=$Tool;tenant_id=$tenant;task_id=$task;target='owned:harness';input=$InputData}|ConvertTo-Json -Depth 16 -Compress) -SkipHttpErrorCheck
 Check-Harness ([int]$r.StatusCode -eq $Status) 'A scoped tool denial did not match the expected status.'
}
$request=@{request_id=[guid]::NewGuid().ToString();script='printf ONCE >> count.txt; printf SAVED > stage.txt; sleep 32; cat count.txt > final.txt';outputs=@('final.txt');timeout_seconds=60}
$clock=[Diagnostics.Stopwatch]::StartNew()
$started=Agent-Harness 'shell.exec' $request
Check-Harness ($started.status -eq 'running' -and $clock.Elapsed.TotalSeconds -lt 10) 'Long work did not return an asynchronous job handle.'
Check-Harness ((Agent-Harness 'shell.exec' $request).job_id -eq $started.job_id) 'Start replay launched another job.'
$other=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$task;work_order_id=$orders[1];targets=@('owned:harness')}).token
Deny-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();op='poll';job_id=$started.job_id} $other 403
Deny-Harness 'work_order.complete' @{outcome='partial';summary='Do not abandon a running job';evidence=@();artifacts=@()} $agent 400
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(2)
do {
 $done=Agent-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();op='poll';job_id=$started.job_id;wait_seconds=20}
 if($done.status -eq 'completed'){break}
 if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Owned long job timed out.'}
}while($true)
Check-Harness ($done.exit_code -eq 0 -and -not $done.timed_out -and $clock.Elapsed.TotalSeconds -ge 31) 'Work was cut off at the old 30-second boundary.'
$body=Agent-Harness 'artifact.read' @{artifact_id=$done.artifacts[0].id}
Check-Harness ($body.content -eq 'ONCE') 'The confirmed long job ran more than once.'
$isolated=Agent-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();script=('cat /jobs/'+$order+'/stage.txt');outputs=@();timeout_seconds=5} $other
Check-Harness ($isolated.exit_code -ne 0) 'A different Work Order read the persistent workspace.'
$continued=Agent-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();script='test "$(cat stage.txt)" = SAVED; printf WORKSPACE_OK > check.txt';outputs=@('check.txt');timeout_seconds=5}
Check-Harness ($continued.exit_code -eq 0) 'The Work Order workspace did not survive the next command.'
$cancelled=Agent-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();op='start';script='sleep 120';outputs=@();timeout_seconds=180}
$null=Agent-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();op='cancel';job_id=$cancelled.job_id}
$cancelResult=Agent-Harness 'shell.exec' @{request_id=[guid]::NewGuid().ToString();op='poll';job_id=$cancelled.job_id;wait_seconds=10}
Check-Harness ($cancelResult.status -eq 'cancelled') 'Explicit cancellation did not settle the job.'
$null=Agent-Harness 'work_order.complete' @{outcome='completed';summary='Owned files and cancellation verified';evidence=@();artifacts=@($done.artifacts[0].id,$continued.artifacts[0].id)}
Push-Location $script:ProjectRoot
try {
 $native=Get-Content (Join-Path $PSScriptRoot 'fixtures/verify-mcp-sessions.py') -Raw | docker compose exec -T mcp-host python3 -
 if($LASTEXITCODE -ne 0){throw 'Real stdio session lifecycle verification failed.'}
 Check-Harness (($native|ConvertFrom-Json).passed) 'MCP session lifecycle verification failed.'
}finally{Pop-Location}
}
$run=$null
if($WithOAuth){
 $admin=(Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id
 $tagA='검증:하네스첫단계';$tagB='검증:하네스후속단계'
 foreach($doc in @(
  @{category='info';title='Owned first harness specification';storage_mode='excerpt';sources=@(@{uri='https://example.com/echelon-owned/harness-first'});tags=@($tagA);content="For independent owned harness checks: the first stage creates stage-one.txt containing the follow-up lookup tag $tagB. Record eight distinct ordinal task.observe checkpoints; only the eighth is an important stage-ready event with needs_handler:true, the earlier routine checkpoints use false. Verify the file and return partial. Handler must collect the real first result, then ask Analyst for a supplemental Pack using that tag. The follow-up source defines the final value."},
  @{category='info';title='Owned follow-up harness specification';storage_mode='excerpt';sources=@(@{uri='https://example.com/echelon-owned/harness-followup'});tags=@($tagB);content='For independent owned harness checks: HARNESS_SECOND_VALUE=73. The second stage writes harness-final.txt with HARNESS_VERIFIED_73 and verifies its saved bytes. This value is only a synthetic fixture, not a company operating rule.'}
 )){
  $submission=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.submit';input=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$doc;rationale='Owned harness fixture specification'}}
  $null=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.publish';input=@{submission_id=$submission.submission.id;decision='accept';reuse=(New-FixtureReuse $submission.submission.document);reason='Owned independent harness fixture'}}
 }
 $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal="Owned synthetic harness verification, with no external actions or credentials. Use $tagA only for the INITIAL Analyst lookup. Follow its first-stage instructions in a FIRST Work Order: eight separate ordinal task.observe calls, first seven needs_handler:false and eighth needs_handler:true for the important stage-ready finding, create and verify stage-one.txt, complete partial. Handler should continue this productive Agent when supervising. Handler must collect that result and only THEN use run.context_refresh to request the follow-up tag from the actual first-stage source/file. Analyst selects the follow-up existing source. In a SECOND Work Order, use that source's value to create and verify harness-final.txt. Complete only when both required stages and saved artifacts are verified. Keep retention and dreaming disabled. All business input is supplied; no human input is needed."}
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(25)
 do {
  $run=Invoke-Echelon "/v1/operator/runs/$($launch.run_id)"
  Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/harness-live-latest.json') ($run|ConvertTo-Json -Depth 40)
  if($run.status -notin @('queued','running')){break}
  if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Owned harness LLM run wait expired; retained for inspection.'}
  Start-Sleep -Seconds 3
 }while($true)
 Check-Harness ($run.status -eq 'completed' -and $run.result.outcome -eq 'completed') "Owned LLM harness result: $($run.status), $($run.error_code)"
 Check-Harness ($run.context_generation -ge 1) 'Handler never requested supplemental central knowledge.'
 Check-Harness (@($run.events|Where-Object {$_.kind -eq 'handler.supervision' -and $_.details.reason -eq 'agent_requested_direction'}).Count -ge 1) 'Important finding did not wake Handler.'
 Check-Harness (@($run.events|Where-Object {$_.kind -eq 'handler.supervision' -and $_.details.reason -eq 'progress_boundary'}).Count -eq 0) 'Routine observations triggered fixed action-count supervision.'
 Check-Harness (@($run.events|Where-Object {$_.kind -eq 'tool.pending' -and $_.details.tool -eq 'run.request_input'}).Count -eq 0) 'The fully specified fixture asked for user input.'
 Check-Harness (@($run.working_state.goals|Where-Object {-not $_.evidence.Count}).Count -eq 0) 'Original goal conditions lack validated evidence IDs.'
 $result=Invoke-Echelon "/v1/operator/tasks/$($launch.task_id)?tenant_id=$tenant"
 Check-Harness ($result.contract.max_work_orders -eq 0 -and $result.work_orders.Count -ge 2) 'Automatic work kept a lifetime cap or skipped a required stage.'
 $final=@($result.artifacts|Where-Object name -eq 'harness-final.txt')[-1]
 $bytes=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$launch.task_id;tool='artifact.read';input=@{artifact_id=$final.id;offset=0;limit=4000}}
 Check-Harness ($bytes.content -match 'HARNESS_VERIFIED_73') 'The saved output does not contain the second source value.'
 Check-Harness ($result.work_orders[0].result.outcome -eq 'partial' -and $result.work_orders[-1].result.outcome -eq 'completed') 'The two actual Work Order outcomes were not preserved.'
 Check-Harness ($result.work_orders[0].contract.references[0].pack_id -ne $result.work_orders[-1].contract.references[0].pack_id) 'The follow-up did not pin a different Context Pack.'
 if($OAuthOnly){$task=$launch.task_id}
}
$result=@{passed=$true;assertions=$checks;tenant_id=$tenant;task_id=$task;async_over_30_seconds=(-not $OAuthOnly);workspace_continuation=(-not $OAuthOnly);work_order_scope=(-not $OAuthOnly);explicit_cancel=(-not $OAuthOnly);duplicate_execution_prevented=(-not $OAuthOnly);stdio_session_reuse=(-not $OAuthOnly);live_oauth=[bool]$WithOAuth;run_id=$(if($run){$run.id}else{$null});verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot $(if($OAuthOnly){'evidence/harness-llm.json'}else{'evidence/harness-native.json'})) ($result|ConvertTo-Json -Depth 10)
$result|ConvertTo-Json -Compress
