param([switch]$RestartCore)
. (Join-Path $PSScriptRoot 'Common.ps1')
$script:ContextAssertions=0
function Assert-Context {
    param([bool]$Condition,[string]$Description)
    if (-not $Condition) {throw $Description};$script:ContextAssertions++
}
function New-ContextTask {
    param([string]$Tenant)
    Invoke-Echelon -Path '/v1/operator/tasks' -Method POST -Body @{tenant_id=$Tenant;tenant_name='Phase 2 owned synthetic context verification';goal='Frozen context verification goal'}
}
function New-ContextSession {
    param([string]$Role,$Context)
    (Invoke-Echelon -Path '/v1/operator/role-tokens' -Method POST -Body @{role=$Role;tenant_id=$Context.tenant_id;task_id=$Context.task_id}).token
}
$tenant=[guid]::NewGuid().ToString()
$writerTask=New-ContextTask $tenant
$handlerTask=New-ContextTask $tenant
$analystTask=New-ContextTask $tenant
$foreignTask=New-ContextTask ([guid]::NewGuid().ToString())
$writer=New-ContextSession handler $writerTask
$handler=New-ContextSession handler $handlerTask
$analyst=New-ContextSession analyst $analystTask
$foreignHandler=New-ContextSession handler $foreignTask
$foreignAnalyst=New-ContextSession analyst $foreignTask
function Invoke-ContextTool {
    param([string]$Tool,$InputData,[string]$Session=$analyst,$Context=$analystTask)
    Invoke-Echelon -Path '/v1/tools/invoke' -Method POST -Token $Session -Body @{tool=$Tool;tenant_id=$Context.tenant_id;task_id=$Context.task_id;input=$InputData}
}
function Assert-ContextDenied {
    param([string]$Tool,$InputData,[int]$Status,[string]$Session=$analyst,$Context=$analystTask,[string]$Code)
    $body=@{tool=$Tool;tenant_id=$Context.tenant_id;task_id=$Context.task_id;input=$InputData} | ConvertTo-Json -Depth 16 -Compress
    $response=Invoke-WebRequest -Method POST -Uri ($script:ApiBase+'/v1/tools/invoke') -Headers @{Authorization=('Bearer '+$Session)} -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) -SkipHttpErrorCheck
    Assert-Context ([int]$response.StatusCode -eq $Status) "Expected context rejection ($Status)."
    if ($Code) {Assert-Context (($response.Content | ConvertFrom-Json).error.code -eq $Code) 'Unexpected context rejection reason.'}
}
function New-ContextSource {
    @{category='data';title='가상 사용 설명서';storage_mode='reference';tags=@('검증:합성','시스템:예시');sources=@(@{uri='https://example.com/context-manual';locator='Synthetic reference only'})}
}
function Register-ContextKnowledge {
    param($Document,[string]$TargetId,[int]$BaseVersion)
    $inputData=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$Document;rationale='Owned context fixture'}
    if ($TargetId) {$inputData.target_id=$TargetId;$inputData.base_version=$BaseVersion}
    $submission=Invoke-ContextTool 'knowledge.submit' $inputData $writer $writerTask
    Invoke-ContextTool 'knowledge.publish' @{submission_id=$submission.submission.id;decision='accept';reason='Registered evidence for context verification'}
}
function Request-Context {
    param($Extra=@{},[string]$Key=([guid]::NewGuid().ToString()))
    $criteria=@{need='가상 보고서를 위한 근거와 과거 실패';tags=@('시스템:예시');constraints=@('Use owned synthetic information only')}
    foreach($name in $Extra.Keys) {$criteria[$name]=$Extra[$name]}
    Invoke-ContextTool 'context.request' @{request_id=$Key;criteria=$criteria} $handler $handlerTask
}
function Context-Delivery {
    param($Request,[string]$Item,[int]$Version=1)
    @{op='deliver';request_id=$Request.request.id;selections=@(@{item_id=$Item;version=$Version;reason='Registered rule and its evidence'});gaps=@()}
}
function Read-Context {
    param($InputData,[string]$Session=$handler,$Context=$handlerTask)
    Invoke-ContextTool 'context.read' $InputData $Session $Context
}

$root=(Register-ContextKnowledge (New-ContextSource)).submission.item_id
$ruleDocument=@{category='info';title='가상 보고 규칙';storage_mode='excerpt';content='CONTEXT_RULE_BODY_VERSION_ONE';tags=@('시스템:예시');relations=@(@{kind='derived_from';target_id=$root;target_version=1})}
$rule=(Register-ContextKnowledge $ruleDocument).submission.item_id
$failed=New-ContextSource
$failed.truth_state='refuted';$failed.outcome='failed'
$failed.assessment=@{conditions='Owned fixture account';attempted_action='Generate a synthetic report';observed='CONTEXT_FAILURE_DETAIL';reason='Fixture grant absent';avoid='Repeat with the same grant';retry_when='A valid grant is independently established'}
$null=Register-ContextKnowledge $failed $root 1
$null=Register-ContextKnowledge (New-ContextSource) $root 2
$failed.title='Related separate failure'
$separate=(Register-ContextKnowledge $failed).submission.item_id
$failed.tags=@('scope:unrelated')
$unrelated=(Register-ContextKnowledge $failed).submission.item_id
$counterDocument=@{category='intel';title='Registered counter-evidence';storage_mode='excerpt';content='CONTEXT_COUNTER_EVIDENCE_BODY';tags=@('scope:counter');relations=@(@{kind='contradicts';target_id=$rule;target_version=1})}
$counter=(Register-ContextKnowledge $counterDocument).submission.item_id
$key=[guid]::NewGuid().ToString()
$request=Request-Context @{} $key
Assert-Context ($request.request.status -eq 'pending') 'Handler request was not queued.'
Assert-Context ((Request-Context @{} $key).replayed) 'Context request retry was not idempotent.'
Assert-ContextDenied 'context.request' @{request_id=$key;criteria=@{need='Changed request';tags=@('시스템:예시')}} 409 $handler $handlerTask
Assert-ContextDenied 'context.request' @{request_id=[guid]::NewGuid().ToString();criteria=@{need='Analyst must not impersonate Handler';tags=@('시스템:예시')}} 403
$queue=Invoke-ContextTool 'context.curate' @{op='queue';status='pending'}
Assert-Context (@($queue.requests).Count -eq 1 -and $queue.requests[0].id -eq $request.request.id) 'Analyst did not receive the request.'
Assert-ContextDenied 'context.curate' @{op='request';request_id=$request.request.id} 403 $handler $handlerTask
Assert-ContextDenied 'context.curate' @{op='request';request_id=$request.request.id} 404 $foreignAnalyst $foreignTask
Assert-ContextDenied 'context.read' @{op='request';request_id=$request.request.id} 404 $writer $writerTask
$candidates=Invoke-ContextTool 'context.curate' @{op='candidates';request_id=$request.request.id;limit=100}
Assert-Context (@($candidates.candidates | Where-Object {$_.item_id -eq $root -and $_.version -eq 2}).Count -eq 1) 'Candidate search hid a historical failure.'
Assert-Context (-not (($candidates | ConvertTo-Json -Depth 16 -Compress).Contains('CONTEXT_RULE_BODY_VERSION_ONE'))) 'Candidate previews exposed full content.'
$keywordRequest=Request-Context @{query='보고'}
$keywordCandidates=Invoke-ContextTool 'context.curate' @{op='candidates';request_id=$keywordRequest.request.id}
Assert-Context (@($keywordCandidates.candidates).Count -eq 1 -and $keywordCandidates.candidates[0].item_id -eq $rule) 'Korean keyword selection returned unrelated knowledge.'
$literalRequest=Request-Context @{query='%'}
$literalCandidates=Invoke-ContextTool 'context.curate' @{op='candidates';request_id=$literalRequest.request.id}
Assert-Context (@($literalCandidates.candidates).Count -eq 0) 'A literal percent query acted as a wildcard.'
$delivery=Context-Delivery $request $rule
$delivered=Invoke-ContextTool 'context.curate' $delivery
$pack=$delivered.pack
$headers=@($pack.knowledge)+@($pack.cautions)
Assert-Context ($pack.reference_count -eq 6) 'Evidence closure did not include the expected fixed versions.'
Assert-Context ($pack.goal -eq 'Frozen context verification goal') 'The Task goal was not pinned.'
Assert-Context (@($headers | Where-Object item_id -eq $unrelated).Count -eq 0) 'Unrelated knowledge entered the pack.'
Assert-Context (@($pack.cautions | Where-Object {$_.item_id -eq $root -and $_.version -eq 2}).Count -eq 1) 'Historical failure was omitted from cautions.'
Assert-Context (@($pack.cautions | Where-Object item_id -eq $separate).Count -eq 1) 'A matching separate failure was omitted.'
Assert-Context (@($pack.cautions | Where-Object {$_.item_id -eq $counter -and $_.reason_codes -contains 'contradiction_related'}).Count -eq 1) 'Reverse counter-evidence was not marked as caution.'
$manifest=$pack | ConvertTo-Json -Depth 16 -Compress
Assert-Context (((Invoke-ContextTool 'context.curate' @{op='pack';pack_id=$pack.id}).pack | ConvertTo-Json -Depth 16 -Compress) -eq $manifest) 'Analyst could not inspect the delivered pack.'
Assert-ContextDenied 'context.curate' @{op='pack';pack_id=$pack.id} 404 $foreignAnalyst $foreignTask
Assert-ContextDenied 'context.curate' @{op='pack';pack_id=$pack.id} 403 $handler $handlerTask
Assert-Context (-not $manifest.Contains('CONTEXT_RULE_BODY_VERSION_ONE') -and -not $manifest.Contains('CONTEXT_FAILURE_DETAIL')) 'Pack manifest included full bodies.'
$replayed=Invoke-ContextTool 'context.curate' $delivery
Assert-Context ($replayed.replayed -and $replayed.pack.id -eq $pack.id) 'Delivery retry changed the pack.'
$fetched=Read-Context @{op='pack';pack_id=$pack.id}
Assert-Context (($fetched.pack | ConvertTo-Json -Depth 16 -Compress) -eq $manifest) 'Handler did not receive the fixed manifest.'
Assert-ContextDenied 'knowledge.read' @{op='get';item_id=$rule;version=1} 403 $handler $handlerTask
$ruleRef=@($headers | Where-Object item_id -eq $rule)[0].ref_id
$resolved=Read-Context @{op='resolve';pack_id=$pack.id;ref_id=$ruleRef}
Assert-Context ($resolved.document.content -eq 'CONTEXT_RULE_BODY_VERSION_ONE' -and $resolved.reference.version -eq 1) 'The selected rule did not resolve to its pinned version.'
Assert-Context ($resolved.reference.document_sha256 -eq @($headers | Where-Object ref_id -eq $ruleRef)[0].document_sha256) 'Reference document hash changed.'
Assert-Context (@($resolved.links).Count -eq 1) 'A pinned evidence link was not granted.'
$evidence=Read-Context @{op='resolve';pack_id=$pack.id;ref_id=$resolved.links[0].ref_id}
Assert-Context ($evidence.reference.item_id -eq $root -and $evidence.reference.version -eq 1) 'Evidence traversal switched to the current version.'
$failureRef=@($headers | Where-Object {$_.item_id -eq $root -and $_.version -eq 2})[0].ref_id
$failureDetail=Read-Context @{op='resolve';pack_id=$pack.id;ref_id=$failureRef}
Assert-Context ($failureDetail.document.assessment.observed -eq 'CONTEXT_FAILURE_DETAIL' -and $failureDetail.document.tags -contains '실패') 'Handler could not inspect preserved failure evidence.'
foreach($scope in @(@{token=$writer;context=$writerTask},@{token=$foreignHandler;context=$foreignTask})) {
    Assert-ContextDenied 'context.read' @{op='pack';pack_id=$pack.id} 404 $scope.token $scope.context
    Assert-ContextDenied 'context.read' @{op='resolve';pack_id=$pack.id;ref_id=$ruleRef} 404 $scope.token $scope.context
}
Assert-ContextDenied 'context.read' @{op='resolve';pack_id=$pack.id;ref_id=[guid]::NewGuid().ToString()} 404 $handler $handlerTask
Assert-ContextDenied 'context.read' @{op='resolve';pack_id=$pack.id;ref_id=$ruleRef;version=999} 400 $handler $handlerTask
$ruleDocument.content='CONTEXT_RULE_BODY_VERSION_TWO'
$null=Register-ContextKnowledge $ruleDocument $rule 1
$old=Read-Context @{op='resolve';pack_id=$pack.id;ref_id=$ruleRef}
Assert-Context ($old.document.content -eq 'CONTEXT_RULE_BODY_VERSION_ONE') 'Source update changed the delivered body.'
Assert-Context ($old.live_notice.version_changed -and $old.live_notice.current_version -eq 2) 'Source change did not produce a separate live notice.'
Assert-Context (((Read-Context @{op='pack';pack_id=$pack.id}).pack | ConvertTo-Json -Depth 16 -Compress) -eq $manifest) 'Source update mutated the manifest.'

$next=Request-Context
$nextDelivery=Context-Delivery $next $rule
$endpoint=$script:ApiBase+'/v1/tools/invoke'
$tenantId=$analystTask.tenant_id;$taskId=$analystTask.task_id
$parallelBody=@{tool='context.curate';tenant_id=$tenantId;task_id=$taskId;input=$nextDelivery} | ConvertTo-Json -Depth 12 -Compress
$results=@(1..2 | ForEach-Object -Parallel {
    $reply=Invoke-RestMethod -Method POST -Uri $using:endpoint -Headers @{Authorization=('Bearer '+$using:analyst)} -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($using:parallelBody))
    @{pack_id=$reply.pack.id;replayed=$reply.replayed}
} -ThrottleLimit 2)
Assert-Context ($results.Count -eq 2 -and $results[0].pack_id -eq $results[1].pack_id -and $results[0].replayed -ne $results[1].replayed) 'Concurrent delivery created duplicate packs.'
Assert-ContextDenied 'context.read' @{op='resolve';pack_id=$results[0].pack_id;ref_id=$ruleRef} 404 $handler $handlerTask
foreach($budget in @(@{extra=@{max_references=1};code='context_reference_budget_exceeded'},@{extra=@{max_manifest_bytes=1024};code='context_manifest_budget_exceeded'})) {
    $small=Request-Context $budget.extra
    Assert-ContextDenied 'context.curate' (Context-Delivery $small $rule) 409 $analyst $analystTask $budget.code
    $state=Read-Context @{op='request';request_id=$small.request.id}
    Assert-Context ($state.request.status -eq 'pending' -and -not $state.request.pack_id) 'A failed budget check left a partial pack.'
}
$empty=Request-Context @{tags=@('scope:no-match')}
$noEvidence=Invoke-ContextTool 'context.curate' @{op='deliver';request_id=$empty.request.id;selections=@();gaps=@('No registered evidence matches the scope')}
Assert-Context ($noEvidence.pack.reference_count -eq 0 -and @($noEvidence.pack.gaps).Count -eq 1) 'Missing information was not explicitly returned as a gap.'
if($RestartCore) {
    Invoke-Compose @('restart','core')
    $ready=$false
    for($attempt=0;$attempt -lt 30;$attempt++) {
        try {$ready=(Invoke-RestMethod -Uri ($script:ApiBase+'/readyz') -TimeoutSec 2).status -eq 'ready';if($ready){break}}
        catch {Start-Sleep -Milliseconds 500}
    }
    Assert-Context $ready 'Core did not recover after restart.'
    Assert-Context (((Read-Context @{op='pack';pack_id=$pack.id}).pack | ConvertTo-Json -Depth 16 -Compress) -eq $manifest) 'Pack did not survive restart.'
    Assert-Context ((Read-Context @{op='resolve';pack_id=$pack.id;ref_id=$ruleRef}).document.content -eq 'CONTEXT_RULE_BODY_VERSION_ONE') 'Pinned reference did not survive restart.'
}
$report=@{verified_at=[DateTimeOffset]::UtcNow.ToString('o');api_assertions=$script:ContextAssertions;request_delivery_verified=$true;references_only_manifest=$true;failure_history_preserved=$true;counter_evidence_included=$true;version_pinning=$true;scoped_resolution=$true;budget_atomicity=$true;concurrent_delivery_idempotent=$true;restart_verified=[bool]$RestartCore}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/context-packs.json') ($report | ConvertTo-Json)
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/context-pack-inventory.json') (@{tenant_id=$tenant;writer_task_id=$writerTask.task_id;handler_task_id=$handlerTask.task_id;analyst_task_id=$analystTask.task_id;pack_id=$pack.id;rule_ref_id=$ruleRef;synthetic_records_retained=$true} | ConvertTo-Json)
$report | ConvertTo-Json -Compress
