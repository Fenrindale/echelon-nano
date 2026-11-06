param([switch]$WithOAuth,[switch]$Resume)
. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Continuity verification requires the isolated port 18432 copy.'}
if((docker compose config --format json|ConvertFrom-Json).name -ne 'echelon-nano-dragdrop-verification'){throw 'Wrong verification project.'}
if(-not $WithOAuth){throw 'Use -WithOAuth: this verifier requires actual role inference.'}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$checks=0
function Check-Continuity([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
function Sql-Continuity([string]$Sql){$v=$Sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon;if($LASTEXITCODE -ne 0){throw 'Owned verification query failed'};return "$v".Trim()}
$activePath=Join-Path $script:RuntimeRoot 'evidence/continuity-active.json'
if($Resume){
 $saved=Get-Content $activePath -Raw|ConvertFrom-Json -AsHashtable
 $tenant=$saved.tenant;$task=$saved.task;$run=$saved.run;$middle=$saved.middle;$tail=$saved.tail;$body=$saved.body;$expected=$saved.expected
}else{
 $tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned large-output continuity verification'}).id
 $middle='MIDDLE_'+[guid]::NewGuid().ToString('N');$tail='TAIL_'+[guid]::NewGuid().ToString('N')
 $rows=@(0..5999|ForEach-Object{@{id=$_;payload='Owned synthetic evidence. Owned synthetic evidence.';marker=$(if($_ -eq 3000){$middle}elseif($_ -eq 5999){$tail}else{''})}})
 $body=@{content=@(@{type='text';text=(@{items=$rows}|ConvertTo-Json -Depth 6 -Compress)})}|ConvertTo-Json -Depth 8 -Compress
 $expected='CONTINUITY_OK|'+$middle+'|'+$tail+'|6000'
 Check-Continuity ([Text.Encoding]::UTF8.GetByteCount($body) -gt 196608 -and [Text.Encoding]::UTF8.GetByteCount($body) -lt 1048576) 'Fixture does not cross the old context cutoff within artifact storage.'
 $goal='Owned synthetic continuity test. Agent first records task.observe observation CONTINUITY_STARTED, then analyzes the attached source.json. It contains MCP-style content[0].text with JSON items. Read the actual source, find total items and the nonempty marker at id 3000 and at the final item. Return continuity.txt containing EXACTLY CONTINUITY_OK|middle-marker|last-marker|total-count with no newline. Marker values must come from source, never be invented. Before finishing record a task.observe with the actual selected IDs and next/completed step so progress is durable. Use partial JSON queries and protected output references as needed. Do not copy the entire JSON through shell/chat or repeatedly reread the entire Work Order. No external network, credentials or central knowledge creation is needed. Retention/audit are optional and should stay disabled. A model round/summary is continuation, not business completion. Verify the actual file bytes.'
 $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal=$goal;attachments=@(@{request_id=[guid]::NewGuid().ToString();name='source.json';media_type='application/json';content=$body})}
 $task=$launch.task_id;$run=$launch.run_id
 $saved=@{tenant=$tenant;task=$task;run=$run;middle=$middle;tail=$tail;body=$body;expected=$expected;goal=$goal;forced_compaction=$false}
 Write-PrivateText $activePath ($saved|ConvertTo-Json -Depth 8)
}
if(-not $saved.forced_compaction){
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(12)
 do{$state=Invoke-Echelon "/v1/operator/runs/$run";$seen=@($state.events|Where-Object {$_.role -eq 'agent' -and $_.kind -eq 'tool.completed' -and $_.details.tool -eq 'task.observe'});if($seen.Count){break};if($state.status -notin @('running','queued')){throw "Owned run stopped before progress: $($state.status),$($state.error_code)"};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Progress boundary timeout'};Start-Sleep -Milliseconds 350}while($true)
 $null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='pause'}
 do{$state=Invoke-Echelon "/v1/operator/runs/$run";if($state.status -eq 'paused'){break};if($state.status -notin @('running','queued')){throw 'Owned run ended before controlled compaction'};Start-Sleep -Milliseconds 300}while($true)
 $saved.original_goal=Sql-Continuity "SELECT state->>'goal' FROM execution_runs WHERE id='$run';"
 $saved.original_conditions=Sql-Continuity "SELECT (state->'memory'->'goals')::text FROM execution_runs WHERE id='$run';"
 $saved.order=$state.work_order_id
 # Owned paused fixture only: force a round boundary, not a business effect or a model result.
 $null=Sql-Continuity "UPDATE execution_runs SET state=jsonb_set(state,'{round_start_calls}',to_jsonb(calls-COALESCE((state->'settings'->>'max_calls')::int,48))) WHERE id='$run' AND tenant_id='$tenant' AND status='paused';"
 $saved.forced_compaction=$true;Write-PrivateText $activePath ($saved|ConvertTo-Json -Depth 8)
 $null=Invoke-Echelon "/v1/operator/runs/$run/control" 'POST' @{action='resume'}
}
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(25)
do{$state=Invoke-Echelon "/v1/operator/runs/$run";Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/continuity-run.json') ($state|ConvertTo-Json -Depth 35);if($state.status -notin @('running','queued')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Owned continuity run timeout'};Start-Sleep -Seconds 2}while($true)
Check-Continuity ($state.status -eq 'completed' -and $state.result.outcome -eq 'completed') "Continuity run failed: $($state.status),$($state.error_code)"
Check-Continuity (@($state.events|Where-Object kind -eq 'context.compacted').Count -ge 1) 'No actual compaction occurred.'
Check-Continuity (@($state.events|Where-Object {$_.kind -eq 'model.completed' -and $_.details.phase -eq 'context_summary'}).Count -ge 1) 'No actual restricted summary inference completed.'
Check-Continuity ((Sql-Continuity "SELECT state->>'goal' FROM execution_runs WHERE id='$run';") -ceq $saved.original_goal) 'Compaction changed the original goal.'
$beforeGoals=$saved.original_conditions|ConvertFrom-Json
Check-Continuity ((@($state.working_state.goals|ForEach-Object condition)|ConvertTo-Json -Compress) -ceq (@($beforeGoals|ForEach-Object condition)|ConvertTo-Json -Compress)) 'Original completion conditions changed.'
$taskState=Invoke-Echelon "/v1/operator/tasks/$task`?tenant_id=$tenant"
$files=@($taskState.artifacts|Where-Object {$_.work_order_id -and $_.name -eq 'continuity.txt'})
Check-Continuity ($files.Count -eq 1) 'Output file was lost or duplicated.'
$file=Invoke-Echelon "/v1/operator/artifacts/$($files[0].id)?tenant_id=$tenant"
Check-Continuity ($file.content -ceq $expected) 'Actual output did not contain the exact source markers/count.'
$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($file.content))).ToLowerInvariant()
Check-Continuity ($hash -ceq $file.artifact.sha256) 'Final artifact bytes differ from immutable hash.'
$source=@($taskState.artifacts|Where-Object {$_.name -eq 'source.json' -and -not $_.work_order_id})
$original=Invoke-Echelon "/v1/operator/artifacts/$($source[0].id)?tenant_id=$tenant"
Check-Continuity ($original.content -ceq $body) 'Source bytes changed during processing.'
Check-Continuity (@($taskState.work_orders).Count -eq 1 -and $taskState.work_orders[0].id -eq $saved.order) 'Compaction duplicated/replaced the active Work Order.'
Check-Continuity ([int](Sql-Continuity "SELECT count(*) FROM execution_output_refs WHERE run_id='$run';") -gt 0) 'Protected output references were not persisted.'
Check-Continuity ([int](Sql-Continuity "SELECT count(*) FROM execution_context_summaries WHERE run_id='$run';") -gt 0) 'Summary was not preserved.'
Check-Continuity ([int](Sql-Continuity "SELECT count(*) FROM observations WHERE task_id='$task';") -ge 2) 'Agent progress notes were lost.'
$result=@{passed=$true;assertions=$checks;run_id=$run;task_id=$task;actual_summary=$true;source_bytes=[Text.Encoding]::UTF8.GetByteCount($body);original_preserved=$true;goal_preserved=$true;progress_preserved=$true;work_order_preserved=$true;file_hash=$hash;calls=$state.calls;tokens=$state.total_tokens;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/continuity.json') ($result|ConvertTo-Json -Depth 6)
$result|ConvertTo-Json -Compress
