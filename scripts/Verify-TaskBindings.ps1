param([switch]$Live,[switch]$ResumeLive)
. (Join-Path $PSScriptRoot 'Common.ps1')
$script:ApiBase='http://127.0.0.1:18432'
$project=(& docker compose config --format json|ConvertFrom-Json).name
if($project -ne 'echelon-nano-dragdrop-verification'){throw 'Use the separate verification project on port 18432.'}
if($ResumeLive -and -not $Live){throw '-ResumeLive requires -Live.'}
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$assertions=0
function Confirm-Binding([bool]$Condition,[string]$Message){if(-not $Condition){throw $Message};$script:assertions++}
function Expect-BindingDenial([scriptblock]$Action,[int]$Status){$denied=$false;try{$null=& $Action}catch{$denied=[int]$_.Exception.Response.StatusCode -eq $Status};Confirm-Binding $denied 'Expected Task binding rejection.'}
$fixture=@"
JENKINS_URL=http://fixture:8081
JENKINS_USER=owned-user
JENKINS_API_TOKEN=owned-identity-token
EMPTY=
"@
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned Task-local identity bindings'}).id
$task=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned Task bindings';goal='Read the configured Jenkins identity and store the owned credential input'}).task_id
$source=(Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$task;artifact=@{request_id=[guid]::NewGuid().ToString();name='env.txt';content=$fixture}}).artifact.id
$binding=@{kind='jenkins';base_url='http://fixture:8081';username_variable='JENKINS_USER';token_variable='JENKINS_API_TOKEN';allow_private=$true;allow_http=$true;source_artifact_id=$source;target_variable='JENKINS_URL'}
$contract=@{kind='work';allowed_tools=@('credential.import','credential.check','artifact.write');input_artifacts=@($source);credential_bindings=@($binding);max_work_orders=2;output_format='Metadata report'}
$null=Invoke-Echelon "/v1/operator/tasks/$task" 'PATCH' @{tenant_id=$tenant;contract=$contract;reason='Select Task-local connection from the owned input'}
$order=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task;tool='agent.delegate';input=@{request_id=[guid]::NewGuid().ToString();target='owned:task-binding';objective='Read the fixed native identity';expected_output='Stored receipt and observed identity status'}}
$agent=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$task;work_order_id=$order.work_order_id;targets=@('owned:task-binding')}).token
function Binding-Tool([string]$Tool,$InputData){Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$task;tool=$Tool;target='owned:task-binding';input=$InputData} $agent}
$inventory=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task;tool='credential.inspect';input=@{artifact_id=$source}}
Confirm-Binding ($inventory.manifest.targets.Count -eq 1 -and $inventory.manifest.targets[0].base_url -eq 'http://fixture:8081') 'Handler did not receive the configured non-secret endpoint.'
Confirm-Binding (-not ($inventory|ConvertTo-Json -Depth 10).Contains('owned-identity-token')) 'Credential inventory disclosed a secret.'
$stored=Binding-Tool 'credential.import' @{artifact_id=$source}
Confirm-Binding ($stored.storage_receipt.audit_seq -gt 0 -and $stored.storage_receipt.details.vault_roundtrip_verified) 'Import produced no server storage receipt.'
$checked=Binding-Tool 'credential.check' @{bundle_id=$stored.bundle.id;binding_index=0}
Confirm-Binding ($checked.observation.authenticated -and $checked.binding_snapshot.base_url -eq $binding.base_url) 'Task-local native identity check failed without a saved profile.'
$replayed=Binding-Tool 'credential.check' @{bundle_id=$stored.bundle.id;binding_index=0}
Confirm-Binding ($replayed.check_id -eq $checked.check_id) 'Task-local identity observation was repeated.'
Expect-BindingDenial {Binding-Tool 'credential.check' @{bundle_id=$stored.bundle.id;binding_index=1}} 403
Expect-BindingDenial {Binding-Tool 'credential.check' @{bundle_id=$stored.bundle.id;binding_index=0;url='https://example.com'}} 400
$evidence=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task;tool='work_order.read';input=@{work_order_id=$order.work_order_id}}
Confirm-Binding ($evidence.tool_evidence.source -eq 'server_records' -and $evidence.tool_evidence.credential_imports.Count -eq 1) 'Handler cannot independently read the storage receipt.'
Confirm-Binding ($evidence.tool_evidence.credential_checks[0].check_id -eq $checked.check_id) 'Handler cannot independently read the actual identity observation.'
Confirm-Binding (-not ($evidence|ConvertTo-Json -Depth 20).Contains('owned-identity-token')) 'Work Order evidence disclosed a secret.'
$listing=Invoke-Echelon "/v1/operator/credentials?tenant_id=$tenant"
Confirm-Binding ($listing.connectors.Count -eq 0) 'Task-local execution created a registry entry.'
$wrong=@{}+$binding;$wrong.base_url='http://other-fixture:8081'
$badContract=@{}+$contract;$badContract.credential_bindings=@($wrong)
Expect-BindingDenial {Invoke-Echelon "/v1/operator/tasks/$task" 'PATCH' @{tenant_id=$tenant;contract=$badContract;reason='Owned target mismatch test'}} 403
$future=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned bundle reuse';goal='Reuse the existing Task credential metadata and configured endpoint'}).task_id
$futureContract=@{kind='work';allowed_tools=@('credential.check');credential_bundles=@($stored.bundle.id);credential_bindings=@($binding);max_work_orders=1}
$null=Invoke-Echelon "/v1/operator/tasks/$future" 'PATCH' @{tenant_id=$tenant;contract=$futureContract;reason='Handler-selected existing bundle and its configuration source'}
$next=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$future;tool='agent.delegate';input=@{request_id=[guid]::NewGuid().ToString();target='owned:task-binding';objective='Read the existing bundle identity';expected_output='Actual status only'}}
$futureToken=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$future;work_order_id=$next.work_order_id;targets=@('owned:task-binding')}).token
$futureCheck=Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$future;tool='credential.check';target='owned:task-binding';input=@{bundle_id=$stored.bundle.id;binding_index=0}} $futureToken
Confirm-Binding $futureCheck.observation.authenticated 'An existing workspace bundle required manual profile registration to be reused.'
$revoked=@{}+$contract;$revoked.credential_bindings=@()
$null=Invoke-Echelon "/v1/operator/tasks/$task" 'PATCH' @{tenant_id=$tenant;contract=$revoked;reason='Owned binding revocation test'}
Expect-BindingDenial {Binding-Tool 'credential.check' @{bundle_id=$stored.bundle.id;binding_index=0}} 403

if($Live){
    $activePath=Join-Path $script:RuntimeRoot 'evidence/task-bindings-live-active.json'
    if($ResumeLive){$launch=Get-Content $activePath -Raw|ConvertFrom-Json}else{
        $liveTenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned LLM Task bindings without registration'}).id
        $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$liveTenant;request_id=[guid]::NewGuid().ToString();goal='첨부 설정의 Jenkins 계정이 동작하는지 확인하고, 기밀을 Vault에 보관한 다음 비밀값 없는 한국어 결과 보고서 파일을 만들어 주세요.';attachments=@(@{request_id=[guid]::NewGuid().ToString();name='env.txt';content=$fixture})}
        $launch|Add-Member -NotePropertyName tenant_id -NotePropertyValue $liveTenant
        Write-PrivateText $activePath ($launch|ConvertTo-Json -Depth 5)
    }
    $deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
    do{
        $run=Invoke-Echelon "/v1/operator/runs/$($launch.run_id)"
        Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/task-bindings-live-run.json') ($run|ConvertTo-Json -Depth 32)
        if($run.status -notin @('queued','running')){break}
        if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Task binding LLM workflow timed out.'}
        Start-Sleep -Seconds 2
    }while($true)
    Confirm-Binding ($run.status -eq 'completed' -and $run.result.outcome -eq 'completed') 'A supported Task without registry entries did not finish the requested work.'
    foreach($tool in @('credential.import','credential.check','artifact.write')){
        Confirm-Binding (@($run.events|Where-Object {$_.role -eq 'agent' -and $_.kind -eq 'tool.completed' -and $_.details.tool -eq $tool}).Count -gt 0) ('Agent did not execute '+$tool)
    }
    $saved=Invoke-Echelon "/v1/operator/tasks/$($launch.task_id)?tenant_id=$($launch.tenant_id)"
    Confirm-Binding ($saved.contract.credential_bindings.Count -gt 0 -and $saved.contract.credential_connectors.Count -eq 0) 'Handler did not configure its own Task-local connection.'
    $catalog=Invoke-Echelon "/v1/operator/credentials?tenant_id=$($launch.tenant_id)"
    Confirm-Binding ($catalog.connectors.Count -eq 0 -and $catalog.checks.Count -gt 0) 'The workflow required or created a connector registry entry.'
    $actual=Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$launch.tenant_id;task_id=$launch.task_id;tool='work_order.read';input=@{work_order_id=$run.work_order_id}}
    Confirm-Binding ($actual.tool_evidence.credential_imports.Count -gt 0 -and $actual.tool_evidence.credential_checks[0].observation.authenticated) 'Independent Handler receipts did not match actual storage and authentication.'
    Confirm-Binding ($run.result.summary.Contains($actual.tool_evidence.credential_checks[0].check_id)) 'Handler omitted the observed identity record from its final assessment.'
    $outputs=@($saved.artifacts|Where-Object work_order_id)
    Confirm-Binding ($outputs.Count -gt 0) 'No actual report was recovered.'
    foreach($output in $outputs){$body=Invoke-Echelon "/v1/operator/artifacts/$($output.id)?tenant_id=$($launch.tenant_id)";Confirm-Binding (-not $body.content.Contains('owned-identity-token')) 'A report disclosed a fixture secret.'}
    $sql="SELECT count(*) FROM execution_runs WHERE id='$($launch.run_id)' AND position('owned-identity-token' IN state::text)>0;"
    $leaks=$sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon
    Confirm-Binding ($LASTEXITCODE -eq 0 -and "$leaks".Trim() -eq '0') 'The LLM checkpoint contains a fixture secret.'
}
$result=@{passed=$true;assertions=$assertions;workspace_registration_required=$false;task_binding_scope_verified=$true;handler_server_receipts_verified=$true;live=$Live.IsPresent;live_run=if($Live){$launch.run_id}else{$null};verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/task-bindings.json') ($result|ConvertTo-Json -Depth 5)
$result|ConvertTo-Json -Compress
