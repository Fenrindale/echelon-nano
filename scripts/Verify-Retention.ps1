. (Join-Path $PSScriptRoot 'Common.ps1')
. (Join-Path $PSScriptRoot 'KnowledgeFixture.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Run semantic retention verification only in the isolated port-18432 copy.'}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned semantic knowledge retention verification'}).id
$task=Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned retention fixture';goal='Review existing fixture knowledge for concrete future decision utility. Preserve situations locally.'}
$scope=@{tenant_id=$tenant;task_id=$task.task_id}
function Retention-Domain([string]$Tool,$InputData){Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task.task_id;tool=$Tool;input=$InputData}}
$docs=@(
 @{category='data';title='Task A deployment status';storage_mode='excerpt';content='At 10:03 Task A build 17 failed with SMTP 535. At 10:10 its owner was notified. At 10:15 the Task was paused. This record describes only this incident; it provides no reusable procedure or decision rule.';sources=@(@{uri='https://example.com/owned/incident'})},
 @{category='info';title='Task A progress';storage_mode='excerpt';content='Task A processed seven of ten rows on Tuesday. Three rows remained pending at the time of this report. This is a one-time progress report with no reusable configuration or workflow.';sources=@(@{uri='https://example.com/owned/progress'})},
 @{category='info';title='SMTP authentication evidence and retry criterion';storage_mode='excerpt';content='When an SMTP server returns 535 before DATA, delivery has not been established. Do not repeat unchanged AUTH or treat TCP reachability as message-delivery proof. Retry only after authentication configuration changes; then verify a successful native send, MIME content and mailbox receipt independently.';sources=@(@{uri='https://example.com/owned/mail-rule'})},
 @{category='data';title='CLI configuration file reference';storage_mode='reference';content='';sources=@(@{uri='https://example.com/owned/cli-configuration-manual';locator='Credential and configuration file precedence';note='Stable fixture documentation reference to consult before selecting a CLI profile'})}
)
$items=@()
foreach($doc in $docs){
 $s=Retention-Domain 'knowledge.submit' @{op='submit';request_id=[guid]::NewGuid().ToString();document=$doc;rationale='Owned legacy-quality fixture, deliberately including nonreusable incidents'}
 # This models the previous overbroad intake; actual semantic review must demote it.
 $a=Retention-Domain 'knowledge.publish' @{submission_id=$s.submission.id;decision='accept';reason='Seed owned legacy fixture';reuse=(New-FixtureReuse $doc)}
 $items+=@{id=$a.submission.item_id;source=$doc}
}
$connection=@((Invoke-Echelon '/v1/auth/connections').connections)[0]
$model=@((Invoke-Echelon "/v1/auth/connections/$($connection.id)/models").models)[0].slug
$request=$scope.Clone();$request.request_id=[guid]::NewGuid().ToString();$request.connection_id=$connection.id;$request.model=$model
$job=(Invoke-Echelon '/v1/operator/knowledge-reviews' 'POST' $request).id
if(-not (Invoke-Echelon '/v1/operator/knowledge-reviews' 'POST' $request).replayed){throw 'Review creation replay was not idempotent.'}
$control=$scope.Clone();$control.action='pause'
$null=Invoke-Echelon "/v1/operator/knowledge-reviews/$job/control" 'POST' $control
$path="/v1/operator/knowledge-reviews/$job"+'?tenant_id='+$tenant+'&task_id='+$task.task_id
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
do{
 $state=Invoke-Echelon $path
 if($state.status -in @('paused','completed','failed')){break}
 if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Review pause deadline exceeded'}
 Start-Sleep -Seconds 2
}while($true)
if($state.status -eq 'paused'){$control.action='resume';$null=Invoke-Echelon "/v1/operator/knowledge-reviews/$job/control" 'POST' $control}
do{
 $state=Invoke-Echelon $path
 if($state.status -notin @('queued','running')){break}
 if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Review completion deadline exceeded'}
 Start-Sleep -Seconds 2
}while($true)
if($state.status -ne 'completed' -or $state.completed -ne 4 -or $state.central -ne 2 -or $state.local -ne 2){throw ('Unexpected semantic review outcome: '+($state|ConvertTo-Json -Compress))}
for($i=0;$i -lt $items.Count;$i++){
 $read=Retention-Domain 'knowledge.read' @{op='get';item_id=$items[$i].id}
 $expected=if($i -lt 2){'task'}else{'central'}
 if($read.item.version -ne 2 -or $read.retention_scope -ne $expected){throw 'Review placement/version mismatch'}
 if($read.item.document.content -ne $items[$i].source.content){throw 'Analyst changed business content'}
 $history=Retention-Domain 'knowledge.read' @{op='history';item_id=$items[$i].id}
 if(@($history.revisions).Count -ne 2){throw 'Prior revision missing'}
}
$central=Retention-Domain 'knowledge.read' @{op='list'}
$local=Retention-Domain 'knowledge.read' @{op='list';retention_scope='task';source_task_id=$task.task_id}
if(@($central.items).Count -ne 2 -or @($local.items).Count -ne 2){throw 'Central/local search placement mismatch'}
$metrics=Invoke-Echelon ('/v1/operator/metrics?tenant_id='+$tenant+'&task_id='+$task.task_id)
if(($metrics.roles|Measure-Object total_tokens -Sum).Sum -ne $state.total_tokens -or $state.total_tokens -lt 1){throw 'Review usage was not measured'}
$analyst=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='analyst';tenant_id=$tenant;task_id=$task.task_id}).token
$denied=Invoke-WebRequest ($script:ApiBase+$path) -Headers @{Authorization='Bearer '+$analyst} -SkipHttpErrorCheck
if([int]$denied.StatusCode -ne 401){throw 'Role bearer gained operator review control'}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$evidence=@{passed=$true;job_id=$job;tenant_id=$tenant;task_id=$task.task_id;state=$state;local_incidents=2;central_decision_rule_and_reference=2;content_unchanged=$true;history_preserved=$true;model_review=$true;token_policy='observe_only'}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/retention.json') ($evidence|ConvertTo-Json -Depth 15)
$evidence|ConvertTo-Json -Depth 6 -Compress
