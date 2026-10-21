. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Run retention-loop fixtures only in the isolated port-18432 copy.'}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned real role retention-loop verification'}).id
$goal='Owned synthetic fixture only. Create report.md with exactly these two sections. Reusable operating rule: When the owned fixture signals WAIT_FOR_CONFIG, do not repeat an unchanged fixture action. Retry only after the fixture configuration revision changes, and compare the actual receipt with the expected output before claiming recovery. This is a fixture-specific instruction for future owned verification Tasks, not a fact about a real system. Task-local incident: Task Q processed seven temporary rows at 10:03 on Tuesday and then paused. This incident has no continuing configuration, preference or causal lesson. No external request or real business action is needed. Create and verify the UTF-8 file using the installed artifact tools. After business completion, retain only the concrete repeatable rule if it qualifies; keep the one-time incident in this Task report, never turn it into central knowledge. Do not invent a generalized lesson or require central knowledge to finish.'
$task=Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned retention-loop fixture';goal=$goal;contract=@{kind='work';allowed_tools=@('artifact.write');output_format='report.md';completion_conditions=@('An immutable report.md includes both provided sections');verification='Read the saved UTF-8 artifact and verify its two sections';max_work_orders=2}}
$connection=@((Invoke-Echelon '/v1/auth/connections').connections)[0]
$model=@((Invoke-Echelon "/v1/auth/connections/$($connection.id)/models").models)[0].slug
$request=@{request_id=[guid]::NewGuid().ToString();tenant_id=$tenant;task_id=$task.task_id;connection_id=$connection.id;model=$model;target='owned:retention-loop';max_calls=48;retain_knowledge=$true;dream_after=$false}
$run=(Invoke-Echelon '/v1/operator/runs' 'POST' $request).id
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/retention-loop-inventory.json') (@{run_id=$run;tenant_id=$tenant;task_id=$task.task_id;request=$request}|ConvertTo-Json -Depth 12)
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
do{
 $state=Invoke-Echelon "/v1/operator/runs/$run"
 if($state.status -notin @('queued','running')){break}
 if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Retention-loop deadline exceeded'}
 Start-Sleep -Seconds 3
}while($true)
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/retention-loop.json') ($state|ConvertTo-Json -Depth 32)
if($state.status -ne 'completed' -or $state.result.outcome -ne 'completed'){throw "Retention loop stopped: $($state.status) $($state.error_code)"}
$domain=@{tenant_id=$tenant;task_id=$task.task_id;tool='knowledge.read';input=@{op='submissions';source_task_id=$task.task_id;limit=100}}
$sub=(Invoke-Echelon '/v1/operator/domain' 'POST' $domain).submissions
$adopted=@($sub|Where-Object retention_scope -eq 'central')
if($adopted.Count -lt 1){throw 'The explicit reusable fixture rule was not retained'}
foreach($entry in $adopted){
 $domain.input=@{op='get';item_id=$entry.item_id}
 $doc=(Invoke-Echelon '/v1/operator/domain' 'POST' $domain).item.document
 if(-not $doc.reuse -or $doc.content -match 'seven temporary rows|seven rows|10:03|Task Q processed'){throw 'Central record contains a one-off situation or lacks grounded reuse metadata'}
 if($doc.content -notmatch 'WAIT_FOR_CONFIG|fixture configuration revision'){throw 'Central record lacks the concrete reusable decision criterion'}
}
foreach($role in @('handler','analyst','agent')){if(-not @($state.events|Where-Object {$_.role -eq $role -and $_.kind -eq 'model.completed'}).Count){throw "Missing actual $role inference"}}
if(-not @($state.events|Where-Object {$_.kind -eq 'tool.completed' -and $_.details.tool -eq 'knowledge.publish' -and $_.role -eq 'analyst'}).Count){throw 'No actual Analyst adoption tool receipt'}
@{passed=$true;run_id=$run;central=$adopted.Count;model_calls=$state.calls;total_tokens=$state.total_tokens;business_completed=$true;situational_report_remains_local=$true;role_owned_retention_and_adoption=$true;token_policy='observe_only'}|ConvertTo-Json -Compress
