param([switch]$Resume)
. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Handoff verification requires the isolated port 18432 copy.'}
if((docker compose config --format json|ConvertFrom-Json).name -ne 'echelon-nano-dragdrop-verification'){throw 'Wrong verification project.'}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$checks=0
function Check-Handoff([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
function Sql-Handoff([string]$Sql){$v=$Sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon;if($LASTEXITCODE -ne 0){throw 'Owned handoff query failed'};return "$v".Trim()}
$active=Join-Path $script:RuntimeRoot 'evidence/handoff-active.json'
if($Resume){$saved=Get-Content $active -Raw|ConvertFrom-Json -AsHashtable}else{
 $tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned two-stage handoff verification'}).id
 $marker='HANDOFF_'+[guid]::NewGuid().ToString('N')
 $body=@{marker=$marker;control="prefix$([char]0)suffix";rows=@(0..11|ForEach-Object{@{index=$_;value="owned row $_"}})}|ConvertTo-Json -Depth 6 -Compress
 $goal=@'
Perform an owned two-stage evidence production and independent review using exactly two separate Agent Work Orders. No external services, credentials, MCP or central knowledge creation. Disable retention/audit.
Stage 1 only: read source.json marker with artifact.read format:json pointer:/marker. Use run.output op:list tool:artifact.read to identify that read's protected original-output UUID and include the exact UUID in your result summary. Read /rows with 12 different offsets 0 through 11, limit:1, in independent native read batches. Also read /control format:json; its NUL must return a reversible json_escaped projection, not an error. Save intermediate.json with the actual marker as field marker. Then task.observe {observation:"Evidence ready for independent review",needs_handler:true}. Handler should continue this productive Agent so it can complete stage 1 with the saved artifact and exact marker-output UUID. Stage 1 must NOT create final.txt or perform stage 2.
Handler: collect the CURRENT Work Order, preserve the original goal, replan, and issue a SECOND Work Order. Pin intermediate.json in input_artifacts and the exact protected marker-output UUID in input_outputs. Do not rerun stage 1, copy sources to repair missing grants, or ask the user.
Stage 2: read the pinned intermediate.json AND run.output on the granted previous Agent output with pointer:/result/value. Compare the actual markers. Save final.txt containing exactly the marker, one pipe, and 12 (no spaces/newline). Read back the file, observe review completion, and complete the assigned work. Handler verifies both stages and the actual final file, then completes the original goal.
'@
 $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal=$goal;attachments=@(@{request_id=[guid]::NewGuid().ToString();name='source.json';media_type='application/json';content=$body})}
 $saved=@{tenant=$tenant;task=$launch.task_id;run=$launch.run_id;marker=$marker;body=$body;expected=($marker+'|12');goal=$goal}
 Write-PrivateText $active ($saved|ConvertTo-Json -Depth 7)
}
$tenant=$saved.tenant;$task=$saved.task;$run=$saved.run
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
do{$r=Invoke-Echelon "/v1/operator/runs/$run";Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/handoff-run.json') ($r|ConvertTo-Json -Depth 60);if($r.status -notin @('queued','running')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Owned handoff run timeout'};Start-Sleep -Seconds 2}while($true)
Check-Handoff ($r.status -eq 'completed' -and $r.result.outcome -eq 'completed') "Handoff workflow failed: $($r.status),$($r.error_code)"
$t=Invoke-Echelon ("/v1/operator/tasks/{0}?tenant_id={1}" -f $task,$tenant)
Check-Handoff (@($t.work_orders).Count -eq 2) 'Exactly two separate Work Orders were required.'
Check-Handoff (@($t.work_orders|Where-Object status -ne 'completed').Count -eq 0) 'A stage did not complete.'
$last=@($t.work_orders|Where-Object {$_.contract.input_outputs.Count -gt 0 -and $_.contract.handoff_artifacts.Count -gt 0})
Check-Handoff ($last.Count -eq 1) 'Prior artifact/output handoff was not pinned.'
$order=$last[0].id
$sql="SELECT count(*) FROM execution_events e JOIN execution_actions a ON a.run_id=e.run_id AND a.call_id=e.details->>'call_id' JOIN execution_output_refs x ON x.run_id=a.run_id AND x.call_id=a.call_id WHERE e.run_id='$run' AND e.kind='tool.completed' AND e.role='agent' AND a.request->>'tool'='run.output' AND x.work_order_id='$order' AND a.request->'input'->>'id' IN (SELECT g->>'id' FROM work_orders w CROSS JOIN LATERAL jsonb_array_elements(w.contract->'input_outputs') g WHERE w.id='$order');"
Check-Handoff (([int](Sql-Handoff $sql)) -ge 1) 'Second Agent never actually read the granted previous output.'
Check-Handoff (([int](Sql-Handoff "SELECT count(*) FROM execution_actions WHERE run_id='$run' AND request->>'tool'='artifact.read' AND outcome->'result'->>'encoding'='json_escaped';")) -ge 1) 'NUL projection was not actually stored.'
Check-Handoff (([int](Sql-Handoff "SELECT count(DISTINCT request->'input'->>'offset') FROM execution_actions WHERE run_id='$run' AND request->>'tool'='artifact.read' AND request->'input'->>'pointer'='/rows';")) -ge 12) 'Normal partial-read sequence did not execute.'
Check-Handoff (@($r.events|Where-Object {$_.kind -eq 'handler.supervision' -and $_.details.reason -eq 'progress_boundary'}).Count -eq 0) 'Fixed action-count supervision still fired.'
Check-Handoff (@($r.events|Where-Object {$_.kind -eq 'handler.supervision' -and $_.details.reason -eq 'agent_requested_direction'}).Count -ge 1) 'Explicit important event never woke Handler.'
$files=@($t.artifacts|Where-Object name -eq 'final.txt');Check-Handoff ($files.Count -eq 1) 'Final file was missing or duplicated.'
$file=Invoke-Echelon "/v1/operator/artifacts/$($files[0].id)?tenant_id=$tenant"
Check-Handoff ($file.content -ceq $saved.expected) 'Independent review produced the wrong file bytes.'
$original=@($t.artifacts|Where-Object name -eq 'source.json');$source=Invoke-Echelon "/v1/operator/artifacts/$($original[0].id)?tenant_id=$tenant"
Check-Handoff ($source.content -ceq $saved.body) 'Original NUL-bearing JSON changed.'
Check-Handoff ((Sql-Handoff "SELECT state->>'goal' FROM execution_runs WHERE id='$run';") -ceq $saved.goal) 'Original goal changed.'
$e=@{passed=$true;assertions=$checks;run_id=$run;task_id=$task;calls=$r.calls;total_tokens=$r.total_tokens;work_orders=2;source_preserved=$true;cross_order_reads_verified=$true;nul_projection_stored=$true;event_supervision_verified=$true;final_sha256=$file.artifact.sha256;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/handoff.json') ($e|ConvertTo-Json -Depth 7)
$e|ConvertTo-Json -Compress
