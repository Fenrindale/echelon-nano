param([ValidateSet('InvalidContext','ExplicitResume')][string]$Scenario='InvalidContext')
. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Generation recovery fixtures require the isolated port 18432 copy.'}
Push-Location $script:ProjectRoot
try {
if((docker compose config --format json|ConvertFrom-Json).name -ne 'echelon-nano-dragdrop-verification'){throw 'Wrong verification project.'}
function Sql-Recovery([string]$Sql){$out=$Sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon;if($LASTEXITCODE -ne 0){throw 'Owned recovery query failed.'};return "$out".Trim()}
if([int](Sql-Recovery "SELECT count(*) FROM execution_runs WHERE status IN ('queued','running');") -ne 0){throw 'Other isolated work is active.'}
$checks=0
function Check-Recovery([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name="Owned generation recovery $Scenario"}).id
$goal='Owned synthetic recovery check. In one Work Order, first create alpha.txt with exact content ALPHA_OK using artifact.write, then read and verify its bytes, then create beta.txt with exact content BETA_OK and read and verify its bytes. Perform these steps sequentially. Do not rewrite an already created file; reuse its immutable ID and receipt. No external resources, secrets or human inputs are required. Completion requires both exact files. Disable retention and audit for this synthetic check.'
$launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal=$goal}
$run=[guid]::Parse($launch.run_id).ToString();$task=[guid]::Parse($launch.task_id).ToString()
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(12)
do {
 $count=[int](Sql-Recovery "SELECT count(*) FROM execution_actions WHERE run_id='$run' AND request->>'tool'='artifact.write' AND outcome IS NOT NULL;")
 if($count -gt 0){break}
 $state=Invoke-Echelon "/v1/operator/runs/$run"
 if($state.status -notin @('queued','running')){throw "Setup stopped: $($state.error_code)"}
 if([DateTimeOffset]::UtcNow -gt $deadline){throw 'First confirmed effect was not reached.'}
 Start-Sleep -Milliseconds 500
} while($true)
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='pause'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(3)
do {$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -eq 'paused'){break};if($state.status -notin @('queued','running') -or [DateTimeOffset]::UtcNow -gt $deadline){throw 'Pause after the first effect failed.'};Start-Sleep -Milliseconds 500} while($true)
# Pause may follow a completed model response with a valid tool decision still
# pending. Execute it normally, then pause during an isolated provider retry.
# Never erase a committed decision to manufacture an empty checkpoint.
$hasPending=[int](Sql-Recovery "SELECT CASE WHEN state->'pending' IS DISTINCT FROM 'null'::jsonb OR jsonb_array_length(COALESCE(state->'read_batch','[]'))>0 THEN 1 ELSE 0 END FROM execution_runs WHERE id='$run';")
if($hasPending) {
 $retriesBefore=[int](Sql-Recovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.retry_scheduled';")
 $network='echelon-nano-dragdrop-verification_provider';$core=(docker compose ps -q core).Trim();$disconnected=$false
 function Internal-RecoveryControl([string]$Action) {
  $payload=@{token=(Get-OperatorToken);run=$run;action=$Action}|ConvertTo-Json -Compress
  $code='import sys,json,urllib.request; p=json.load(sys.stdin); r=urllib.request.Request("http://core:8080/v1/operator/runs/"+p["run"]+"/control",data=json.dumps({"action":p["action"]}).encode(),headers={"Authorization":"Bearer "+p["token"],"Content-Type":"application/json"}); urllib.request.urlopen(r,timeout=30).read(); print("CONTROL_OK")'
  $out=$payload|docker compose exec -T fixture python3 -c $code
  if($LASTEXITCODE -ne 0 -or "$out".Trim() -ne 'CONTROL_OK'){throw 'Internal owned control failed.'}
 }
 try {
  docker network disconnect $network $core
  if($LASTEXITCODE -ne 0){throw 'Isolated boundary setup failed.'};$disconnected=$true
  Internal-RecoveryControl 'resume'
  $deadline=[DateTimeOffset]::UtcNow.AddMinutes(3)
  do {if([int](Sql-Recovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.retry_scheduled';") -gt $retriesBefore){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'No isolated generation boundary was reached.'};Start-Sleep -Milliseconds 500} while($true)
  Internal-RecoveryControl 'pause'
  $deadline=[DateTimeOffset]::UtcNow.AddMinutes(2)
  do {if((Sql-Recovery "SELECT status FROM execution_runs WHERE id='$run';") -eq 'paused'){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Boundary pause failed.'};Start-Sleep -Milliseconds 500} while($true)
 } finally {
  if($disconnected){docker network connect $network $core;if($LASTEXITCODE -ne 0){throw 'RESTORE FAILED: reconnect the isolated provider network.'}}
 }
}
$before=Sql-Recovery "SELECT jsonb_build_object('goal',state->'goal','goals',state->'memory'->'goals','order',state->'order','phase',state->'phase','pending',state->'pending','batch',state->'read_batch','actions',(SELECT jsonb_object_agg(call_id,md5(to_jsonb(a)::text)) FROM execution_actions a WHERE run_id=r.id AND outcome IS NOT NULL)) FROM execution_runs r WHERE id='$run' AND tenant_id='$tenant' AND task_id='$task';" | ConvertFrom-Json -AsHashtable
Check-Recovery ($null -eq $before.pending -and @($before.batch).Count -eq 0) 'A pending or unknown effect cannot be used for this fixture.'
if($Scenario -eq 'InvalidContext') {
 # Alter only this owned paused fixture. The real provider must reject the opaque
 # item; returned model proposals remain subject to normal terminal-response gating.
 $null=Sql-Recovery "UPDATE execution_runs SET state=jsonb_set(state,'{history}',(state->'history')||jsonb_build_array(jsonb_build_object('type','reasoning','id','rs_000000000000000000000000000000000000000000000000','summary','[]'::jsonb,'encrypted_content','owned-corrupt-context-verification'))) WHERE id='$run' AND status='paused' AND tenant_id='$tenant';"
} else {
 # Reproduce a legacy failed checkpoint after all model-only retries were spent.
 # Preserve every confirmed tool action and its original immutable record.
 $null=Sql-Recovery "UPDATE execution_runs SET status='failed',error_code='inference_failed',state=jsonb_set(state,'{model_failures}','3'::jsonb) WHERE id='$run' AND status='paused' AND tenant_id='$tenant';"
}
$null=Sql-Recovery "INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES('$tenant','$task','operator','verification.generation_fault','injected',jsonb_build_object('scenario','$Scenario','run_id','$run','synthetic',true,'business_effect_replayed',false));"
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='resume'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(15)
do {$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -notin @('queued','running')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Generation recovery timed out.'};Start-Sleep -Seconds 2} while($true)
Check-Recovery ($state.status -eq 'completed' -and $state.result.outcome -eq 'completed') "Recovery did not complete: $($state.status) $($state.error_code)"
$after=Sql-Recovery "SELECT jsonb_build_object('goal',state->'goal','goals',state->'memory'->'goals','actions',(SELECT jsonb_object_agg(call_id,md5(to_jsonb(a)::text)) FROM execution_actions a WHERE run_id=r.id AND outcome IS NOT NULL)) FROM execution_runs r WHERE id='$run';" | ConvertFrom-Json -AsHashtable
Check-Recovery ($before.goal -ceq $after.goal) 'The immutable business goal changed.'
foreach($id in $before.actions.Keys){Check-Recovery ($before.actions[$id] -eq $after.actions[$id]) 'A confirmed action or its receipt was changed.'}
$recoveries=[int](Sql-Recovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.context_recovery' AND details->>'effects_replayed'='false' AND details->>'goal_preserved'='true';")
Check-Recovery ($recoveries -ge 1) 'No context rebuild was recorded.'
Check-Recovery ([int](Sql-Recovery "SELECT count(*) FROM execution_context_archives WHERE run_id='$run';") -ge 1) 'The prior context was not archived.'
if($Scenario -eq 'InvalidContext') {
 Check-Recovery ([int](Sql-Recovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.failed' AND details->'diagnostic'->>'source' IN ('http','sse');") -ge 1) 'The actual provider rejection has no safe structural evidence.'
} else {
 Check-Recovery ([int](Sql-Recovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.context_recovery' AND details->>'code'='explicit_resume_after_inference_failure';") -eq 1) 'Explicit resume reused the failed context.'
}
$detail=Invoke-Echelon "/v1/operator/tasks/$task`?tenant_id=$tenant"
foreach($name in @('alpha.txt','beta.txt')) {
 $files=@($detail.artifacts|Where-Object {$_.work_order_id -and $_.name -eq $name})
 Check-Recovery ($files.Count -eq 1) 'Recovery duplicated or lost an output.'
 $file=Invoke-Echelon "/v1/operator/artifacts/$($files[0].id)?tenant_id=$tenant"
 $expected=if($name -eq 'alpha.txt'){'ALPHA_OK'}else{'BETA_OK'}
 Check-Recovery ($file.content -ceq $expected) 'Recovered output bytes do not match the goal.'
}
Check-Recovery ([int](Sql-Recovery "SELECT count(*) FROM execution_actions WHERE run_id='$run' AND request->>'tool'='artifact.write' AND outcome->>'ok'='true';") -eq 2) 'A completed business effect was executed again.'
Check-Recovery ([int](Sql-Recovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.context_recovery' AND details->>'work_order_id'='$($before.order)';") -ge 1) 'Recovery changed the active Work Order.'
$result=@{passed=$true;scenario=$Scenario;assertions=$checks;run_id=$run;calls=$state.calls;recoveries=$recoveries;goal_preserved=$true;confirmed_effects_preserved=$true}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
Write-PrivateText (Join-Path $script:RuntimeRoot "evidence/inference-recovery-$Scenario.json") ($result|ConvertTo-Json)
$result|ConvertTo-Json -Compress
} finally {Pop-Location}
