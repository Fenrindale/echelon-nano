param([switch]$Resume)
. (Join-Path $PSScriptRoot 'Common.ps1')
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$checks=0
function Check-Goal([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
if($Resume){
 $saved=Get-Content (Join-Path $script:RuntimeRoot 'evidence/goal-launch-active.json') -Raw|ConvertFrom-Json -AsHashtable
 $tenant=$saved.tenant_id;$request=$saved.request
 $submission=@{submission=@{item_id=$saved.item_id}}
 $launch=@{task_id=$saved.task_id;run_id=$saved.run_id}
}else{
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned prompt-only Handler verification'}).id
$admin=(Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id
$document=@{category='info';title='가상 진행 보고의 계산 기준';storage_mode='excerpt';content='검증:목표실행의 가상 진행 보고는 전체 10건, 완료 7건, 대기 3건이며 완료율은 70%다. 실제 회사 성과로 일반화하지 않는다.';truth_state='supported';tags=@('검증:목표실행');sources=@(@{uri='https://example.com/echelon-owned/prompt-only-report'})}
$submission=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.submit';input=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$document;rationale='Operator-authored synthetic fixture'}}
$null=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$admin;tool='knowledge.publish';input=@{submission_id=$submission.submission.id;decision='accept';reason='Known owned fixture values'}}
$request=@{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal='검증:목표실행 태그의 기준으로 프로젝트 진행 현황을 간단한 한국어 Markdown 보고서 파일로 만들어 주세요. 전체 10건 중 완료 7건과 대기 3건을 표로 정리하고 완료율을 계산해 주세요. 근거의 정보 ID와 버전을 인용하고, 실제 회사 성과가 아닌 가상 자료라는 한계를 명시해 주세요.'}
$launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' $request
}
$again=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' $request
Check-Goal ($again.replayed -and $again.task_id -eq $launch.task_id -and $again.run_id -eq $launch.run_id) 'A repeated prompt request created a second Task or run.'
$conflict=$false
$changed=@{}+$request
$changed.goal='Different goal under the same immutable request key'
try{$null=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' $changed}catch{$conflict=$_.ErrorDetails.Message -match 'goal_launch_conflict'}
Check-Goal $conflict 'A launch request key accepted a different goal.'
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/goal-launch-active.json') (@{tenant_id=$tenant;task_id=$launch.task_id;run_id=$launch.run_id;item_id=$submission.submission.item_id;request=$request}|ConvertTo-Json -Depth 8)
$deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
do{
 $run=Invoke-Echelon "/v1/operator/runs/$($launch.run_id)"
 Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/goal-launch-run.json') ($run|ConvertTo-Json -Depth 30)
 if($run.status -notin @('queued','running')){break}
 if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Prompt-only live execution timed out.'}
 Start-Sleep -Seconds 2
}while($true)
Check-Goal ($run.status -eq 'completed') "Prompt-only execution stopped: $($run.status), $($run.error_code)"
Check-Goal ($run.result.outcome -eq 'completed') 'The Handler did not complete the actual business goal.'
$task=Invoke-Echelon "/v1/operator/tasks/$($launch.task_id)?tenant_id=$tenant"
Check-Goal ($task.contract.output_format.Length -gt 0 -and @($task.contract.completion_conditions).Count -gt 0 -and $task.contract.verification.Length -gt 0) 'Handler did not derive the business contract.'
Check-Goal ($run.contract_finalized -and $run.handler_settings.model.Length -gt 0) 'Handler did not finalize its execution settings.'
Check-Goal ($run.handler_settings.max_calls -ge 16 -and $run.handler_settings.max_calls -le 48 -and $run.handler_settings.max_total_tokens -le 300000) 'Handler widened the installed budget ceiling.'
foreach($tool in @('run.configure','run.configure_contract')){Check-Goal (@($run.events|Where-Object {$_.kind -eq 'tool.completed' -and $_.details.tool -eq $tool}).Count -eq 1) "Handler configuration effect is missing or duplicated: $tool"}
foreach($role in @('handler','analyst','agent')){Check-Goal (@($run.events|Where-Object {$_.kind -eq 'model.completed' -and $_.role -eq $role}).Count -gt 0) "Missing actual model role: $role"}
$outputs=@($task.artifacts|Where-Object {$_.work_order_id -and ($_.name -like '*.md' -or $_.media_type -eq 'text/markdown')})
Check-Goal ($outputs.Count -gt 0) 'The prompt-only work produced no actual Markdown artifact.'
$file=Invoke-Echelon "/v1/operator/artifacts/$($outputs[-1].id)?tenant_id=$tenant"
foreach($value in @('10','7','3','70')){Check-Goal ($file.content.Contains($value)) "Actual report omitted required value $value"}
$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($file.content))).ToLowerInvariant()
Check-Goal ($hash -eq $file.artifact.sha256) 'Recovered report differs from its immutable hash.'
Check-Goal ($file.content.Contains($submission.submission.item_id) -and $file.content -match 'v1|버전\s*:?\s*1|version\s*:?\s*1') 'Actual report omitted its pinned evidence citation.'
$result=@{passed=$true;assertions=$checks;tenant_id=$tenant;task_id=$launch.task_id;run_id=$launch.run_id;only_business_input='goal';handler_settings=$run.handler_settings;calls=$run.calls;total_tokens=$run.total_tokens;artifact_hash_verified=$true;launch_replay_without_duplicates=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/goal-launch.json') ($result|ConvertTo-Json -Depth 12)
$result|ConvertTo-Json -Depth 12 -Compress
