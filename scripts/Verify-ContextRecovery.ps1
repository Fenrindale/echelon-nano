param([ValidateSet('Both','SmallPack','ReturnedRequest')][string]$Scenario='Both')
. (Join-Path $PSScriptRoot 'Common.ps1')
. (Join-Path $PSScriptRoot 'KnowledgeFixture.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Context recovery fixtures require the isolated port 18432 copy.'}
Push-Location $script:ProjectRoot
try {
if((docker compose config --format json|ConvertFrom-Json).name -ne 'echelon-nano-dragdrop-verification'){throw 'Wrong verification project.'}
function Sql-ContextRecovery([string]$Sql){$value=$Sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon;if($LASTEXITCODE -ne 0){throw 'Owned context query failed.'};return "$value".Trim()}
if([int](Sql-ContextRecovery "SELECT count(*) FROM execution_runs WHERE status IN ('queued','running');") -ne 0){throw 'Other isolated work is active.'}
$checks=0
function Check-ContextRecovery([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned Context overflow and Handler return verification'}).id
$admin=(Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id
$tag='검증:ContextRecovery'+([guid]::NewGuid().ToString('N').Substring(0,8))
for($i=0;$i -lt 20;$i++) {
 $doc=@{category='info';title="Owned historical caution $i";storage_mode='excerpt';content=("Owned fixture warning $i applies only to a retired synthetic action. "+('x'*8000));truth_state='refuted';outcome='failed';tags=@($tag);assessment=@{conditions='Retired synthetic action only';attempted_action='Historical owned fixture';observed='Synthetic failure';reason='Fixture condition';avoid='Replay the retired synthetic action';retry_when='A different independently specified owned action is authorized'};sources=@(@{uri='https://example.com/owned/context-recovery'})}
 $submission=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.submit';input=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$doc;rationale='Owned isolated fixture'}}
 $null=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.publish';input=@{submission_id=$submission.submission.id;decision='accept';reuse=(New-FixtureReuse $submission.submission.document);reason='Owned independent recovery specification'}}
}
$results=@()
foreach($case in @('SmallPack','ReturnedRequest')) {
 if($Scenario -ne 'Both' -and $Scenario -ne $case){continue}
 $goal="[$tag] Owned synthetic context recovery check. Use this exact tag with an empty search query, and explicitly set max_references:1 and max_manifest_bytes:1024 in the Context request. These are display preferences. Analyst must inspect existing candidates and preserve the historical cautions. They describe refuted, retired synthetic actions; none prohibits the separate action requested here. Do not manufacture positive knowledge. Handler must delegate the following fully specified action through the delivered context_pack_id, allowing the server to attach every mandatory caution without enumerating or individually re-reading all warnings: Agent creates context-flow.txt containing exactly CONTEXT_FLOW_OK using artifact.write, reads it and verifies the exact bytes. No external URL, credentials or human input is needed. Do not rewrite an already created file. Retain no new central knowledge and disable audit for this owned action. If a curator returns an internal request, Handler must refine/re-request it autonomously; keep the same goal and completed work. All business inputs are present."
 $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal=$goal}
 $run=[guid]::Parse($launch.run_id).ToString();$task=[guid]::Parse($launch.task_id).ToString()
 if($case -eq 'ReturnedRequest') {
  $deadline=[DateTimeOffset]::UtcNow.AddMinutes(4)
  do {
   $request=Sql-ContextRecovery "SELECT id FROM context_requests WHERE tenant_id='$tenant' AND task_id='$task' ORDER BY created_at LIMIT 1;"
   if($request){break}
   if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Handler did not create its context request.'}
   Start-Sleep -Milliseconds 200
  }while($true)
  # Simulate another authorized curator returning this OWNED request while the
  # actual Analyst is running. Preserve all pending model decisions and effects.
  $returned=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task;tool='context.curate';input=@{op='return';request_id=$request;reason='Owned controlled curator return. Handler must create a new context request for the same fully specified synthetic action.'}}|ConvertTo-Json -Depth 20|ConvertFrom-Json -AsHashtable
  Check-ContextRecovery ($null -ne $returned['returned']) 'Controlled return arrived after delivery; no record was altered.'
 }
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(15)
 do {
  $state=Invoke-Echelon "/v1/operator/runs/$run"
  if($state.status -notin @('queued','running')){break}
  if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Context recovery workflow timed out.'}
  Start-Sleep -Seconds 2
 }while($true)
 Check-ContextRecovery ($state.status -eq 'completed' -and $state.result.outcome -eq 'completed') "Context flow failed: $case $($state.status) $($state.error_code)"
 Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM context_requests WHERE tenant_id='$tenant' AND task_id='$task' AND criteria->>'max_references'='1' AND criteria->>'max_manifest_bytes'='1024';") -ge 1) 'The real Handler did not exercise small display preferences.'
 Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM context_packs WHERE tenant_id='$tenant' AND task_id='$task' AND jsonb_array_length(manifest->'cautions')=20;") -ge 1) 'The delivered Pack lost mandatory failure history.'
 Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM work_orders WHERE tenant_id='$tenant' AND task_id='$task' AND jsonb_array_length(contract->'references')=20 AND octet_length(contract::text)>131072;") -ge 1) 'The Work Order omitted cautions or hit the old fixed evidence cap.'
 Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='tool.failed' AND details#>>'{error,code}' IN ('context_reference_budget_exceeded','context_manifest_budget_exceeded');") -eq 0) 'Display preferences still rejected immutable evidence.'
 Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.completed' AND role='analyst';") -ge 1) 'The actual Analyst did not run.'
 if($case -eq 'ReturnedRequest') {
  Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM execution_events WHERE run_id='$run' AND kind='model.completed' AND role='handler' AND details->>'phase'='context_replan';") -ge 1) 'Analyst did not hand control back to the real Handler.'
  Check-ContextRecovery ([int](Sql-ContextRecovery "SELECT count(*) FROM context_requests WHERE tenant_id='$tenant' AND task_id='$task';") -ge 2) 'Handler reused the returned immutable request.'
 }
 $detail=Invoke-Echelon "/v1/operator/tasks/$task`?tenant_id=$tenant"
 $files=@($detail.artifacts|Where-Object {$_.work_order_id -and $_.name -eq 'context-flow.txt'})
 Check-ContextRecovery ($files.Count -eq 1) 'The business effect was duplicated or omitted.'
 $file=Invoke-Echelon "/v1/operator/artifacts/$($files[0].id)?tenant_id=$tenant"
 Check-ContextRecovery ($file.content -ceq 'CONTEXT_FLOW_OK') 'The recovered output differs from the requested goal.'
 $results+=@{scenario=$case;run_id=$run;calls=$state.calls;passed=$true}
}
$result=@{passed=$true;assertions=$checks;tenant_id=$tenant;scenarios=$results;source_records=20;live_oauth=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
Write-PrivateText (Join-Path $script:RuntimeRoot "evidence/context-recovery-$Scenario.json") ($result|ConvertTo-Json -Depth 8)
$result|ConvertTo-Json -Depth 8 -Compress
} finally {Pop-Location}
