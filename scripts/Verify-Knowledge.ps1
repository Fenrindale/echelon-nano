param([switch]$RestartCore)
. (Join-Path $PSScriptRoot 'Common.ps1')
. (Join-Path $PSScriptRoot 'KnowledgeFixture.ps1')
$script:Assertions = 0
function Assert-Knowledge {
    param([bool]$Condition,[string]$Description)
    if (-not $Condition) { throw $Description }
    $script:Assertions++
}
function New-VerificationTask {
    param([string]$Tenant)
    Invoke-Echelon -Path '/v1/operator/tasks' -Method POST -Body @{
        tenant_id=$Tenant;tenant_name='Phase 1 owned synthetic knowledge verification';goal='Verify knowledge intake and preserved history'
    }
}
function New-RoleSession {
    param([string]$Role,$Context)
    (Invoke-Echelon -Path '/v1/operator/role-tokens' -Method POST -Body @{
        role=$Role;tenant_id=$Context.tenant_id;task_id=$Context.task_id
    }).token
}
$tenant=[guid]::NewGuid().ToString()
$sourceTask=New-VerificationTask $tenant
$curationTask=New-VerificationTask $tenant
$otherTask=New-VerificationTask ([guid]::NewGuid().ToString())
$handler=New-RoleSession handler $sourceTask
$laterHandler=New-RoleSession handler $curationTask
$analyst=New-RoleSession analyst $curationTask
$foreignAnalyst=New-RoleSession analyst $otherTask
function Invoke-KnowledgeTool {
    param([string]$Tool,$InputData,[string]$Session=$analyst,$Context=$curationTask)
    Invoke-Echelon -Path '/v1/tools/invoke' -Method POST -Token $Session -Body @{
        tool=$Tool;tenant_id=$Context.tenant_id;task_id=$Context.task_id;input=$InputData
    }
}
function Assert-KnowledgeDenied {
    param([string]$Tool,$InputData,[int]$Status,[string]$Session=$analyst,$Context=$curationTask)
    $body=@{tool=$Tool;tenant_id=$Context.tenant_id;task_id=$Context.task_id;input=$InputData} | ConvertTo-Json -Depth 16 -Compress
    $response=Invoke-WebRequest -Method POST -Uri ($script:ApiBase+'/v1/tools/invoke') -Headers @{Authorization=('Bearer '+$Session)} -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) -SkipHttpErrorCheck
    Assert-Knowledge ([int]$response.StatusCode -eq $Status) "Expected knowledge API rejection ($Status)."
}
function New-ReferenceDocument {
    @{
        category='data';title='가상 기술문서 위치';storage_mode='reference';tags=@('검증:합성','시스템:예시')
        sources=@(@{uri='https://example.com/manual';locator='synthetic section';note='Reference only; no external fetch'})
    }
}
function Submit-Document {
    param($Document,[string]$TargetId,[int]$BaseVersion,[string]$Session=$handler,$Context=$sourceTask)
    $payload=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$Document;rationale='Reusable synthetic evidence selected by Handler'}
    if ($TargetId) { $payload.target_id=$TargetId;$payload.base_version=$BaseVersion }
    Invoke-KnowledgeTool 'knowledge.submit' $payload $Session $Context
}
function Accept-Submission {
    param($Submission)
    Invoke-KnowledgeTool 'knowledge.publish' @{submission_id=$Submission.submission.id;decision='accept';reason='Analyst adopted the submitted evidence';reuse=(New-FixtureReuse $Submission.submission.document)}
}
function Read-Item { param([string]$Id)
    Invoke-KnowledgeTool 'knowledge.read' @{op='get';item_id=$Id}
}

$document=New-ReferenceDocument
$payload=@{op='submit';request_id=[guid]::NewGuid().ToString();document=$document;rationale='Reference location is sufficient'}
$pending=Invoke-KnowledgeTool 'knowledge.submit' $payload $handler $sourceTask
Assert-Knowledge ($pending.submission.status -eq 'pending') 'Submission was not queued.'
Assert-Knowledge (@((Invoke-KnowledgeTool 'knowledge.read' @{}).items).Count -eq 0) 'An unaccepted submission entered the central store.'
$replayed=Invoke-KnowledgeTool 'knowledge.submit' $payload $handler $sourceTask
Assert-Knowledge ($replayed.replayed -and $replayed.submission.id -eq $pending.submission.id) 'Submission retry was not idempotent.'
$acceptance=@{submission_id=$pending.submission.id;decision='accept';reason='Analyst adopted the submitted evidence';reuse=(New-FixtureReuse $document)}
Assert-KnowledgeDenied 'knowledge.publish' $acceptance 403 $handler $sourceTask
Assert-KnowledgeDenied 'knowledge.submit' $payload 403
$queue=Invoke-KnowledgeTool 'knowledge.read' @{op='submissions';status='pending'}
Assert-Knowledge (@($queue.submissions).Count -eq 1) 'Analyst did not see the cross-Task intake queue.'
Assert-Knowledge ($queue.submissions[0].title -eq '가상 기술문서 위치' -and $queue.submissions[0].PSObject.Properties.Name -notcontains 'document') 'The intake preview exposed the full document.'
$accepted=Accept-Submission $pending
$root=$accepted.submission.item_id
Assert-Knowledge ($accepted.submission.version -eq 1) 'Initial adoption did not create version 1.'
Assert-Knowledge ((Invoke-KnowledgeTool 'knowledge.publish' $acceptance).replayed) 'Adoption replay was not idempotent.'
Assert-Knowledge ([string]::IsNullOrEmpty((Read-Item $root).item.document.content)) 'A reference required copied document content.'

$infoDocument=@{
    category='info';title='가상 운영 규칙';storage_mode='excerpt';content='Short synthetic guidance retained with its source.';tags=@('검증:합성','업무:보고')
    relations=@(@{kind='derived_from';target_id=$root;target_version=1},@{kind='depends_on';target_id=$root;target_version=1})
}
$info= (Accept-Submission (Submit-Document $infoDocument)).submission.item_id
$intelDocument=@{
    category='intel';title='가상 업무 판단';storage_mode='excerpt';content='Use the fixture rule for the synthetic task only.'
    relations=@(@{kind='derived_from';target_id=$info;target_version=1})
}
$intel=(Accept-Submission (Submit-Document $intelDocument)).submission.item_id
$filtered=Invoke-KnowledgeTool 'knowledge.read' @{op='list';category='info';tags=@('업무:보고');query='운영'}
Assert-Knowledge (@($filtered.items).Count -eq 1 -and $filtered.items[0].id -eq $info) 'Category, tag and Korean keyword filtering failed.'
$negative=New-ReferenceDocument
$negative.truth_state='refuted';$negative.outcome='failed'
Assert-KnowledgeDenied 'knowledge.submit' @{op='submit';request_id=[guid]::NewGuid().ToString();document=$negative;rationale='Missing evidence must be rejected'} 400 $handler $sourceTask
$negative.assessment=@{conditions='Owned synthetic account';attempted_action='Read a fixture route';observed='Denied';reason='Fixture grant absent';avoid='Repeat without an authorized grant';retry_when='A new grant is verified'}
$negativeSubmission=Submit-Document $negative $root 1
$null=Accept-Submission $negativeSubmission
$head=Read-Item $root
Assert-Knowledge ($head.item.document.tags -contains '거짓' -and $head.item.document.tags -contains '실패') 'Negative state tags were not synchronized.'
Assert-Knowledge ($head.item.document.truth_state -eq 'refuted' -and $head.item.document.outcome -eq 'failed') 'Truth and outcome were not retained separately.'
$warnings=@((Read-Item $intel).dependency_warnings)
Assert-Knowledge (@($warnings | Where-Object { $_.item_id -eq $root -and $_.current_version -eq 2 }).Count -eq 1) 'A transitive dependency update did not produce a review warning.'
$null=Invoke-KnowledgeTool 'knowledge.classify' @{item_id=$root;expected_version=2;category='info';tags=@('검증:합성');reason='Analyst metadata curation'}
$head=Read-Item $root
Assert-Knowledge ($head.item.version -eq 3 -and $head.item.document.tags -contains '거짓' -and $head.item.document.tags -contains '실패') 'Curation erased the negative state.'
Assert-Knowledge ($head.item.source_task_id -eq $sourceTask.task_id) 'Curation lost the source Task provenance.'
Assert-KnowledgeDenied 'knowledge.classify' @{item_id=$root;expected_version=3;tags=@();content='Forged business fact';reason='Body edits are not curation'} 400
Assert-KnowledgeDenied 'knowledge.classify' @{item_id=$root;expected_version=2;tags=@();reason='Stale write'} 409
$corrected=New-ReferenceDocument
$corrected.truth_state='supported';$corrected.outcome='success'
$null=Accept-Submission (Submit-Document $corrected $root 3 $laterHandler $curationTask)
$old=Invoke-KnowledgeTool 'knowledge.read' @{op='get';item_id=$root;version=2}
Assert-Knowledge ($old.item.document.outcome -eq 'failed' -and $old.item.document.assessment.observed -eq 'Denied') 'The original failure evidence was lost.'
Assert-Knowledge (@((Read-Item $root).negative_history).Count -eq 2) 'Negative revision history disappeared after correction.'
$retained=Invoke-KnowledgeTool 'knowledge.read' @{op='list';negative_history=$true}
Assert-Knowledge (@($retained.items).Count -eq 1 -and $retained.items[0].has_negative_history) 'Corrected failures were not discoverable.'

Assert-KnowledgeDenied 'knowledge.read' @{op='get';item_id=$root} 404 $foreignAnalyst $otherTask
Assert-KnowledgeDenied 'knowledge.publish' $acceptance 404 $foreignAnalyst $otherTask
Assert-KnowledgeDenied 'knowledge.submit' @{op='status';submission_id=$pending.submission.id} 404 $laterHandler $curationTask
$foreignDocument=New-ReferenceDocument
$foreignDocument.relations=@(@{kind='references';target_id=$root;target_version=1})
$foreignHandler=New-RoleSession handler $otherTask
Assert-KnowledgeDenied 'knowledge.submit' @{op='submit';request_id=[guid]::NewGuid().ToString();document=$foreignDocument;rationale='Foreign edge must be rejected'} 400 $foreignHandler $otherTask
$cyclic=New-ReferenceDocument
$cyclic.relations=@(@{kind='depends_on';target_id=$info;target_version=1})
Assert-KnowledgeDenied 'knowledge.submit' @{op='submit';request_id=[guid]::NewGuid().ToString();target_id=$root;base_version=4;document=$cyclic;rationale='Cycle must be rejected'} 400 $handler $sourceTask

$one=Submit-Document (New-ReferenceDocument) $root 4
$two=Submit-Document (New-ReferenceDocument) $root 4 $laterHandler $curationTask
$endpoint=$script:ApiBase+'/v1/tools/invoke'
$analystTenant=$curationTask.tenant_id;$analystTask=$curationTask.task_id
$decisions=@($one.submission.id,$two.submission.id)
$concurrentReuse=New-FixtureReuse (New-ReferenceDocument)
$statuses=@($decisions | ForEach-Object -Parallel {
    $body=@{tool='knowledge.publish';tenant_id=$using:analystTenant;task_id=$using:analystTask;input=@{submission_id=$_;decision='accept';reason='Concurrent adoption verification';reuse=$using:concurrentReuse}} | ConvertTo-Json -Depth 8 -Compress
    $response=Invoke-WebRequest -Method POST -Uri $using:endpoint -Headers @{Authorization=('Bearer '+$using:analyst)} -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) -SkipHttpErrorCheck
    [int]$response.StatusCode
} -ThrottleLimit 2)
Assert-Knowledge (@($statuses | Where-Object { $_ -eq 200 }).Count -eq 1 -and @($statuses | Where-Object { $_ -eq 409 }).Count -eq 1) 'Concurrent adoptions overwrote the same base version.'
Assert-Knowledge ((Read-Item $root).item.version -eq 5) 'Concurrent adoption advanced more than one version.'
$rejected=Submit-Document (New-ReferenceDocument)
$rejection=@{submission_id=$rejected.submission.id;decision='reject';reason='One-off fixture, not reusable'}
$decision=Invoke-KnowledgeTool 'knowledge.publish' $rejection
Assert-Knowledge ($decision.submission.status -eq 'rejected' -and -not $decision.submission.item_id) 'Rejection created central knowledge.'
Assert-KnowledgeDenied 'knowledge.publish' @{submission_id=$rejected.submission.id;decision='accept';reason='Cannot change a final decision'} 409
$status=Invoke-KnowledgeTool 'knowledge.submit' @{op='status';submission_id=$rejected.submission.id} $handler $sourceTask
Assert-Knowledge ($status.submission.document.title -eq '가상 기술문서 위치' -and $status.submission.status -eq 'rejected') 'Rejected evidence was not retained.'

if ($RestartCore) {
    Invoke-Compose @('restart','core')
    $ready=$false
    for ($i=0;$i -lt 30;$i++) {
        try { $ready=(Invoke-RestMethod -Uri ($script:ApiBase+'/readyz') -TimeoutSec 2).status -eq 'ready';if ($ready) { break } }
        catch { Start-Sleep -Milliseconds 500 }
    }
    Assert-Knowledge $ready 'Core did not recover after restart.'
    Assert-Knowledge ((Read-Item $root).item.version -eq 5) 'Central knowledge did not survive restart.'
}
$history=Invoke-KnowledgeTool 'knowledge.read' @{op='history';item_id=$root}
Assert-Knowledge (@($history.revisions).Count -eq 5) 'A central revision was lost.'
$evidence=@{
    verified_at=[DateTimeOffset]::UtcNow.ToString('o');api_assertions=$script:Assertions;categories_verified=@('data','info','intel')
    handler_to_analyst_flow=$true;versioned_tags_and_relations=$true;negative_history_preserved=$true;cross_task_reuse=$true
    tenant_isolation=$true;dependency_warnings=$true;concurrent_version_conflict=$true;rejection_preserved=$true;restart_verified=[bool]$RestartCore
}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/knowledge.json') ($evidence | ConvertTo-Json -Depth 5)
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/knowledge-context.json') (@{tenant_id=$tenant;source_task_id=$sourceTask.task_id;curation_task_id=$curationTask.task_id;item_ids=@($root,$info,$intel);synthetic_records_retained=$true} | ConvertTo-Json)
$evidence | ConvertTo-Json -Depth 5 -Compress
