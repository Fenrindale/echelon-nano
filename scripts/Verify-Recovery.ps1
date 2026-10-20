param([string]$ConnectionId,[string]$Model,[switch]$PromptOnly)
. (Join-Path $PSScriptRoot 'Common.ps1')
Push-Location $script:ProjectRoot
try {
$activeCount=& docker compose exec -T postgres psql -U echelon -d echelon -Atc "SELECT count(*) FROM execution_runs WHERE status IN ('queued','running')"
if($LASTEXITCODE -ne 0 -or [long]("$activeCount".Trim()) -ne 0){throw 'Run recovery verification only when no other Echelon execution is active.'}
$assertions=0
function Assert-Recovery([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:assertions++}
$connections=@((Invoke-Echelon '/v1/auth/connections').connections)
if(-not $ConnectionId){if($connections.Count -ne 1){throw 'Select one connected account.'};$ConnectionId=$connections[0].id}
if(-not $Model){$Model=@((Invoke-Echelon "/v1/auth/connections/$ConnectionId/models").models)[0].slug}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned checkpoint and event replay verification'}).id
$admin=(Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id
$doc=@{category='info';title='Owned recovery fixture procedure';storage_mode='excerpt';truth_state='supported';applicability='Operator-authored synthetic test only; completion is independently checked against the actual Worker receipt and output';content='For tag 검증:재개 this operator-owned synthetic Task is fully specified. Execute its single permitted shell script in the isolated sandbox, wait for the receipt, and return resume.txt. This is a registered Info fixture, not a server protocol. No external evidence or business write is required. If interrupted, the operator can explicitly resume the same run and its existing Work Order.';tags=@('검증:재개');sources=@(@{uri='https://example.com/echelon-owned/recovery'})}
$submission=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.submit';input=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$doc;rationale='Owned recovery fixture'}}
$null=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.publish';input=@{submission_id=$submission.submission.id;decision='accept';reason='Operator registered a bounded synthetic procedure'}}
$definition=@{tenant_id=$tenant;tenant_name='Owned recovery fixture';goal='검증:재개. This is an authorized, fully specified recovery test. Use the registered Info recovery fixture as a pinned reference; a selected server protocol is not required. Agent must execute exactly one shell.exec with script: sleep 12; printf RESUME_OK > resume.txt . The 12-second wait is a required test condition and must not be removed. outputs:[resume.txt],timeout_seconds:25. Return the actual recovered resume.txt artifact and cite the pinned procedure. A replayed existing receipt is the expected successful outcome after explicit resume, not a reason to block. Completion requires the actual receipt and exact output bytes; do not add unrequested protocol-certification requirements. No external action is requested.';contract=@{kind='work';allowed_tools=@('shell.exec');output_format='resume.txt';max_work_orders=1;completion_conditions=@('Actual shell receipt and resume.txt containing RESUME_OK');verification='Compare recovered content to RESUME_OK'}}
if($PromptOnly){
 $request=@{request_id=[guid]::NewGuid().ToString();tenant_id=$tenant;goal=$definition.goal}
 $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' $request
 $task=$launch.task_id;$run=$launch.run_id
}else{
 $task=(Invoke-Echelon '/v1/operator/tasks' 'POST' $definition).task_id
 $request=@{request_id=[guid]::NewGuid().ToString();tenant_id=$tenant;task_id=$task;connection_id=$ConnectionId;model=$Model;target='owned:recovery';max_calls=48;max_total_tokens=180000}
 $run=(Invoke-Echelon '/v1/operator/runs' 'POST' $request).id
}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/recovery-active.json') (@{tenant_id=$tenant;task_id=$task;run_id=$run;request=$request}|ConvertTo-Json -Depth 10)
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='pause'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(5)
do{$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -eq 'paused'){break};if($state.status -notin @('queued','running')){throw "Unexpected pause state: $($state.status)"};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Pause boundary timed out.'};Start-Sleep -Milliseconds 500}while($true)
Assert-Recovery ($state.status -eq 'paused') 'Pause was not applied.'
$beforePause=$state.calls;Start-Sleep -Seconds 1
Assert-Recovery ((Invoke-Echelon "/v1/operator/runs/$run").calls -eq $beforePause) 'Paused execution made another model call.'
$null=Invoke-Echelon "/v1/operator/tasks/$task/inputs" 'POST' @{tenant_id=$tenant;content='Continue the owned recovery test with the same exact script and completion conditions.'}
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='resume'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
$shellPending=$null
do{$state=Invoke-Echelon "/v1/operator/runs/$run";$shellPending=@($state.events|Where-Object {$_.kind -eq 'tool.pending' -and $_.details.tool -eq 'shell.exec'});if($shellPending.Count){break};if($state.status -notin @('queued','running')){throw "Recovery run stopped before shell: $($state.status), $($state.error_code)"};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Shell pending boundary timed out.'};Start-Sleep -Milliseconds 350}while($true)
$beforeKillCalls=$state.calls
$beforeSettings=$state.handler_settings|ConvertTo-Json -Depth 8 -Compress
$runIdentity=[guid]::Parse($run)
$otherCount=& docker compose exec -T postgres psql -U echelon -d echelon -Atc "SELECT count(*) FROM execution_runs WHERE id<>'$runIdentity' AND status IN ('queued','running')"
if($LASTEXITCODE -ne 0 -or [long]("$otherCount".Trim()) -ne 0){throw 'A different Echelon execution became active; the core will not be interrupted.'}
try{
 Invoke-Compose @('kill','-s','SIGKILL','core')
 Start-Sleep -Seconds 2
}finally{Invoke-Compose @('start','core')}
$ready=$false
for($i=0;$i -lt 40;$i++){try{$null=Invoke-RestMethod -Uri ($script:ApiBase+'/readyz') -TimeoutSec 2;$ready=$true;break}catch{Start-Sleep -Milliseconds 500}}
if(-not $ready){throw 'Core did not recover readiness.'}
$interrupted=Invoke-Echelon "/v1/operator/runs/$run"
Assert-Recovery ($interrupted.status -eq 'interrupted') 'Restart did not preserve a stopped checkpoint.'
Assert-Recovery ($interrupted.calls -eq $beforeKillCalls) 'Restart automatically replayed provider inference.'
if($PromptOnly){Assert-Recovery ($interrupted.handler_settings -and (($interrupted.handler_settings|ConvertTo-Json -Depth 8 -Compress) -eq $beforeSettings) -and $interrupted.contract_finalized) 'Restart lost Handler settings or finalized contract.'}
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='resume'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(10)
do{$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -notin @('queued','running')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Resumed run timed out.'};Start-Sleep -Seconds 1}while($true)
Assert-Recovery ($state.status -eq 'completed' -and $state.result.outcome -eq 'completed') 'The resumed run did not complete.'
$taskState=Invoke-Echelon "/v1/operator/tasks/$task`?tenant_id=$tenant"
Assert-Recovery (@($taskState.work_orders).Count -eq 1) 'Restart duplicated a Work Order.'
$receipts=@($taskState.tool_receipts|Where-Object tool -eq 'shell.exec')
Assert-Recovery ($receipts.Count -eq 1 -and $receipts[0].status -eq 'completed') 'Restart duplicated or lost the shell effect receipt.'
Assert-Recovery ($receipts[0].result.replayed) 'Worker did not return the original job receipt after core interruption.'
$output=@($taskState.artifacts|Where-Object {$_.work_order_id -and $_.name -eq 'resume.txt'})
Assert-Recovery ($output.Count -eq 1) 'The resumed effect produced duplicate output artifacts.'
Assert-Recovery ((Invoke-Echelon "/v1/operator/artifacts/$($output[0].id)?tenant_id=$tenant").content -eq 'RESUME_OK') 'Recovered output content differs.'
$workerEvents=@(docker compose logs --no-log-prefix worker|ForEach-Object{try{$_|ConvertFrom-Json}catch{}})
$starts=@($workerEvents|Where-Object {$_.fields.event -eq 'worker_job_started' -and $_.fields.job_id -eq $receipts[0].result.job_id})
Assert-Recovery ($starts.Count -eq 1) 'The Worker executed the recovered job more than once.'
$finishes=@($workerEvents|Where-Object {$_.fields.event -eq 'worker_job_completed' -and $_.fields.job_id -eq $receipts[0].result.job_id})
Assert-Recovery ($finishes.Count -eq 1) 'The original Worker job did not finish exactly once.'
$workerStartedAt=[DateTimeOffset]$starts[0].timestamp;$workerFinishedAt=[DateTimeOffset]$finishes[0].timestamp
$workerElapsed=($workerFinishedAt-$workerStartedAt).TotalSeconds
Assert-Recovery ($workerElapsed -ge 11.5 -and $workerElapsed -lt 35) 'The actual required 12-second wait was not preserved within its bounded timeout.'
# Reconnect from a persisted SSE event cursor in this exact Task.
$http=[Net.Http.HttpClient]::new();$cts=[Threading.CancellationTokenSource]::new(8000)
function Read-RecoveryEvents([long]$After,[int]$Count){
 $message=[Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get,$script:ApiBase+"/v1/operator/events?tenant_id=$tenant&task_id=$task")
 $message.Headers.Authorization=[Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer',(Get-OperatorToken));$null=$message.Headers.TryAddWithoutValidation('Last-Event-ID',"$After")
 $response=$http.SendAsync($message,[Net.Http.HttpCompletionOption]::ResponseHeadersRead,$cts.Token).GetAwaiter().GetResult();$response.EnsureSuccessStatusCode()|Out-Null
 $reader=[IO.StreamReader]::new($response.Content.ReadAsStream());$values=[Collections.Generic.List[object]]::new()
 try{while($values.Count -lt $Count){$line=$reader.ReadLineAsync($cts.Token).AsTask().GetAwaiter().GetResult();if($null -eq $line){break};if($line.StartsWith('data: ')){$event=$line.Substring(6)|ConvertFrom-Json;if($event.PSObject.Properties.Name -contains 'seq'){$values.Add($event)}}}}finally{$reader.Dispose();$response.Dispose();$message.Dispose()}
 $values.ToArray()
}
try{$first=@(Read-RecoveryEvents 0 3);Assert-Recovery ($first.Count -eq 3) 'Persisted SSE activity was not replayed.';$next=@(Read-RecoveryEvents $first[-1].seq 2);Assert-Recovery ($next.Count -eq 2 -and $next[0].seq -gt $first[-1].seq) 'Last-Event-ID did not advance monotonically.';Assert-Recovery (@(@($first)+@($next)|Where-Object task_id -ne $task).Count -eq 0) 'SSE replay leaked another Task.'}finally{$cts.Dispose();$http.Dispose()}
$result=@{passed=$true;prompt_only=[bool]$PromptOnly;assertions=$assertions;tenant_id=$tenant;task_id=$task;run_id=$run;pause_verified=$true;explicit_restart_resume=$true;worker_executions=1;work_orders=1;output_artifacts=1;receipt_replayed=$true;sse_task_scope_and_cursor_verified=$true;calls=$state.calls;total_tokens=$state.total_tokens;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/recovery.json') ($result|ConvertTo-Json -Depth 8)
$result|ConvertTo-Json -Compress
}finally{Pop-Location}
