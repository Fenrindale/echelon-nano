# Controlled faults in an owned synthetic Task, never in the user's working project.
. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Persistence faults require the isolated port 18432 copy.'}
Push-Location $script:ProjectRoot
try {
$project=(docker compose config --format json|ConvertFrom-Json).name
if($project -ne 'echelon-nano-dragdrop-verification'){throw 'Wrong verification project.'}
$active=docker compose exec -T postgres psql -X -U echelon -d echelon -Atc "SELECT (SELECT count(*) FROM execution_runs WHERE status IN ('queued','running'))+(SELECT count(*) FROM knowledge_ingestion_jobs WHERE status IN ('queued','running'));"
if($LASTEXITCODE -ne 0 -or [int]("$active".Trim()) -ne 0){throw 'Stop other isolated verification runs before fault injection.'}
$checks=0
function Check-Persistence([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
function Sql-Persistence([string]$Sql){$out=$Sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon;if($LASTEXITCODE -ne 0){throw 'Owned verification query failed.'};return "$out".Trim()}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned persistence fault verification'}).id
$launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal='Owned synthetic persistence check. Create persistence.txt containing exactly PERSISTENCE_OK and verify its actual bytes using installed artifact tools. Use no external URLs or credentials. All business input is supplied. Keep max_work_orders at least 2. This owned workflow requires TWO sequential Work Orders: in the first Work Order Agent records task.observe confirming the objective, completes partial with a limitation that file creation remains, and does not create a file. Handler must retrieve that actual partial result and replan. While planning the second order, Handler must re-read the first order with work_order.read using its actual previous Work Order ID, preserving the immutable prior result. In the second Work Order Agent creates persistence.txt and verifies its bytes, then completes. The overall goal is achieved only after both stages and the actual verified file exist. Keep retention and audit disabled for this synthetic check.'}
$run=[guid]::Parse($launch.run_id).ToString();$task=[guid]::Parse($launch.task_id).ToString()
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='pause'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(6)
do{$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -eq 'paused'){break};if($state.status -notin @('queued','running')){throw 'Unexpected setup result.'};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Pause timeout.'};Start-Sleep -Seconds 1}while($true)
$before=[int]$state.calls
# Simulate an exhausted MODEL ROUND in this paused owned record, preserving all decisions/effects.
$null=Sql-Persistence "UPDATE execution_runs SET state=jsonb_set(state,'{round_start_calls}',to_jsonb(calls-48)) WHERE id='$run' AND status='paused' AND tenant_id='$tenant';"
$network=$project+'_provider';$core=(docker compose ps -q core).Trim();$disconnected=$false
try {
 docker network disconnect $network $core
 if($LASTEXITCODE -ne 0){throw 'Provider fault injection failed.'};$disconnected=$true
 # HTTP is published through this network. Start and control through the isolated internal fixture.
 function Internal-Control([string]$Action){
  $payload=@{token=(Get-OperatorToken);run=$run;action=$Action}|ConvertTo-Json -Compress
  $code='import sys,json,urllib.request; p=json.load(sys.stdin); r=urllib.request.Request("http://core:8080/v1/operator/runs/"+p["run"]+"/control",data=json.dumps({"action":p["action"]}).encode(),headers={"Authorization":"Bearer "+p["token"],"Content-Type":"application/json"}); urllib.request.urlopen(r,timeout=30).read(); print("CONTROL_OK")'
  $out=$payload|docker compose exec -T fixture python3 -c $code
  if($LASTEXITCODE -ne 0 -or "$out".Trim() -ne 'CONTROL_OK'){throw 'Internal owned run control failed.'}
 }
 Internal-Control 'resume'
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(3)
 do{$retries=[int](Sql-Persistence "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.retry_scheduled';");if($retries -gt 0){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Transient inference did not schedule a retry.'};Start-Sleep -Seconds 1}while($true)
 Check-Persistence ((Sql-Persistence "SELECT status FROM execution_runs WHERE id='$run';") -eq 'running') 'Transient error terminated the Task.'
 Internal-Control 'pause'
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(2)
 do{$status=Sql-Persistence "SELECT status FROM execution_runs WHERE id='$run';";if($status -eq 'paused'){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Pause during retry timed out.'};Start-Sleep -Seconds 1}while($true)
 $pausedCalls=Sql-Persistence "SELECT calls FROM execution_runs WHERE id='$run';"
 Start-Sleep -Seconds 3
 Check-Persistence ((Sql-Persistence "SELECT calls FROM execution_runs WHERE id='$run';") -eq $pausedCalls) 'Paused retry made another model request.'
 Check-Persistence ([int](Sql-Persistence "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='execution.round_started';") -ge 1) 'Automatic Task stopped at its round call limit.'
} finally {if($disconnected){docker network connect $network $core;if($LASTEXITCODE -ne 0){throw 'RESTORE FAILED: reconnect the isolated provider network.'}}}
$null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='resume'}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
do{$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -notin @('queued','running')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Recovered inference timed out.'};Start-Sleep -Seconds 2}while($true)
Check-Persistence ($state.status -eq 'completed' -and $state.result.outcome -eq 'completed') "Recovery failed: status=$($state.status) outcome=$($state.result.outcome) error=$($state.error_code)"
Check-Persistence ($state.result.goal_assessment.goal_met -and @($state.result.goal_assessment.remaining_work).Count -eq 0) 'Handler omitted the goal completion assessment.'
$detail=Invoke-Echelon "/v1/operator/tasks/$task`?tenant_id=$tenant"
$files=@($detail.artifacts|Where-Object {$_.work_order_id -and $_.name -eq 'persistence.txt'})
Check-Persistence ($files.Count -eq 1) 'Recovered work duplicated or lost its output.'
$file=Invoke-Echelon "/v1/operator/artifacts/$($files[0].id)?tenant_id=$tenant"
Check-Persistence ($file.content -ceq 'PERSISTENCE_OK') 'The actual output bytes differ from the goal.'
$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($file.content))).ToLowerInvariant()
Check-Persistence ($hash -eq $file.artifact.sha256) 'The recovered file differs from its immutable hash.'
Check-Persistence (@($detail.work_orders).Count -eq 2) 'Handler did not create exactly the two required sequential Work Orders.'
Check-Persistence ((Sql-Persistence "SELECT string_agg(r.result->>'outcome',',' ORDER BY w.created_at) FROM work_orders w JOIN work_order_results r ON r.work_order_id=w.id WHERE w.task_id='$task';") -eq 'partial,completed') 'Agent results do not preserve the required partial-to-completed sequence.'
Check-Persistence (@($state.events|Where-Object {$_.kind -eq 'tool.completed' -and $_.details.tool -eq 'run.replan'}).Count -eq 1) 'Handler finalized the first partial attempt instead of replanning.'
Check-Persistence (@($state.events|Where-Object {$_.kind -eq 'tool.completed' -and $_.details.tool -eq 'work_order.read' -and $_.details.phase -eq 'delegate'}).Count -ge 1) 'Handler could not re-read its earlier collected Work Order.'
Check-Persistence ([int](Sql-Persistence "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.retry_scheduled';") -ge 1) 'Retry evidence disappeared.'
$evidence=@{assertions=$checks;run_id=$run;tenant_id=$tenant;task_id=$task;calls=$state.calls;round=$state.round;provider_fault='isolated network disconnect';network_restored=$true;result=$state.result}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/persistence.json') ($evidence|ConvertTo-Json -Depth 16)
$evidence|Select-Object assertions,run_id,calls,round,network_restored|ConvertTo-Json -Compress
} finally {Pop-Location}
