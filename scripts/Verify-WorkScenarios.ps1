param([string]$TenantId,[string]$ConnectionId,[string]$Model,[switch]$Resume)
. (Join-Path $PSScriptRoot 'Common.ps1')
$script:ScenarioAssertions=0
$progressPath=Join-Path $script:RuntimeRoot 'evidence/work-scenarios-progress.json'
$progress=@{tasks=@{};runs=@{};configs=@{};started=@{};elapsed=@{};vars=@{};protocol='';pack='';tenant=''}
if($Resume){if(-not(Test-Path -LiteralPath $progressPath)){throw 'No owned scenario checkpoint exists.'};$progress=Get-Content -LiteralPath $progressPath -Raw|ConvertFrom-Json -AsHashtable;$TenantId=$progress.tenant;if(($ConnectionId -and $ConnectionId -ne $progress.connection_id) -or ($Model -and $Model -ne $progress.model)){throw 'Resume must retain the original account and model.'};$ConnectionId=$progress.connection_id;$Model=$progress.model}
function Assert-Scenario([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:ScenarioAssertions++}
if(-not $TenantId){$TenantId=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned beta work scenarios'}).id}
$tenant=$TenantId
$progress.tenant=$tenant
$admin=(Invoke-Echelon "/v1/operator/tenants/$tenant/workspace" 'POST' @{}).administration_task_id
$connections=@((Invoke-Echelon '/v1/auth/connections').connections)
if(-not $ConnectionId){if($connections.Count -ne 1){throw 'Select one explicitly connected account.'};$ConnectionId=$connections[0].id}
$catalog=@((Invoke-Echelon "/v1/auth/connections/$ConnectionId/models").models)
if(-not $Model){$Model=$catalog[0].slug}
$progress.connection_id=$ConnectionId;$progress.model=$Model
function Scenario-Domain([string]$Task,[string]$Tool,$InputData){Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$Task;tool=$Tool;input=$InputData}}
function Scenario-Task([string]$Goal,$Contract){(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned beta scenarios';goal=$Goal;contract=$Contract}).task_id}
function Save-ScenarioProgress {Write-PrivateText ($progressPath+'.tmp') ($progress|ConvertTo-Json -Depth 40);[IO.File]::Move(($progressPath+'.tmp'),$progressPath,$true)}
function Scenario-StageTask([string]$Name,[string]$Goal,$Contract){if($progress.tasks.ContainsKey($Name)){return $progress.tasks[$Name]};$id=Scenario-Task $Goal $Contract;$progress.tasks[$Name]=$id;Save-ScenarioProgress;$id}
function Scenario-Knowledge($Document){$s=Scenario-Domain $admin 'knowledge.submit' @{op='submit';request_id=[guid]::NewGuid().ToString();document=$Document;rationale='Owned synthetic work scenario evidence'};Scenario-Domain $admin 'knowledge.publish' @{submission_id=$s.submission.id;decision='accept';reason='Operator registered the owned fixture source'} }
function Scenario-Upload([string]$Task,[string]$Name,[string]$Content){$hash=[Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Task+':'+$Name));$requestKey=[guid]::new([byte[]]$hash[0..15]);(Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$Task;artifact=@{request_id=$requestKey.ToString();name=$Name;content=$Content}}).artifact.id}
function Scenario-Run([string]$Task,$Extra=@{}){
 $alreadyCompleted=$false
 $request=@{request_id=[guid]::NewGuid().ToString();tenant_id=$tenant;task_id=$Task;connection_id=$ConnectionId;model=$Model;target='owned:work-scenario';max_calls=48;max_total_tokens=300000;retain_knowledge=$false;dream_after=$false}
 foreach($k in $Extra.Keys){$request[$k]=$Extra[$k]}
 if($progress.configs.ContainsKey($Task)){$request=$progress.configs[$Task]}else{$progress.configs[$Task]=$request;$progress.started[$Task]=[DateTimeOffset]::UtcNow.ToString('o');Save-ScenarioProgress}
 if($progress.runs.ContainsKey($Task)){$started=@{id=$progress.runs[$Task]};$existing=Invoke-Echelon "/v1/operator/runs/$($started.id)";$alreadyCompleted=$existing.status -eq 'completed';if($existing.status -in @('failed','paused','interrupted')){$null=Invoke-Echelon "/v1/operator/runs/$($started.id)/control" 'POST' @{action='resume'}}}else{$started=Invoke-Echelon '/v1/operator/runs' 'POST' $request;$progress.runs[$Task]=$started.id;Save-ScenarioProgress}
 Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/scenarios-active.json') (@{tenant_id=$tenant;task_id=$Task;run_id=$started.id;request=$request}|ConvertTo-Json -Depth 20)
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(25)
 do{$r=Invoke-Echelon "/v1/operator/runs/$($started.id)";Write-PrivateText (Join-Path $script:RuntimeRoot "evidence/scenario-$($started.id).json") ($r|ConvertTo-Json -Depth 28);if($r.status -notin @('queued','running')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Scenario execution deadline exceeded.'};Start-Sleep -Seconds 2}while($true)
 if(-not $alreadyCompleted -or -not $progress.elapsed.ContainsKey($Task)){$progress.elapsed[$Task]=[long]([DateTimeOffset]::UtcNow-[DateTimeOffset]::Parse($progress.started[$Task])).TotalMilliseconds};Save-ScenarioProgress
 Assert-Scenario ($r.status -eq 'completed') "Actual scenario stopped: $($r.status), $($r.error_code), $($r.phase). Use -Resume after addressing the recorded cause."
 Assert-Scenario ($r.result.outcome -eq 'completed') 'Scenario did not meet its business completion gate.'
 $r|Add-Member -NotePropertyName elapsed_ms -NotePropertyValue $progress.elapsed[$Task]
 Write-PrivateText (Join-Path $script:RuntimeRoot "evidence/scenario-$($started.id).json") ($r|ConvertTo-Json -Depth 28)
 Write-Host ("Completed owned scenario: {0} calls, {1} tokens, {2} ms" -f $r.calls,$r.total_tokens,$r.elapsed_ms)
 $r
}
function Scenario-Output([string]$Task,[string]$Name){$t=Invoke-Echelon "/v1/operator/tasks/$Task`?tenant_id=$tenant";$files=@($t.artifacts|Where-Object {$_.work_order_id -and $_.name -eq $Name});Assert-Scenario ($files.Count -ge 1) "Missing actual output: $Name";$output=Invoke-Echelon "/v1/operator/artifacts/$($files[-1].id)?tenant_id=$tenant";$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($output.content))).ToLowerInvariant();Assert-Scenario ($hash -eq $output.artifact.sha256) 'Recovered UTF-8 content hash differs from its immutable artifact.';$output}
function Check-ScenarioReport([string]$Task,[string]$Content){$t=Invoke-Echelon "/v1/operator/tasks/$Task`?tenant_id=$tenant";$fetches=@($t.tool_receipts|Where-Object {$_.tool -eq 'external.fetch' -and $_.status -eq 'completed' -and $_.result.status -eq 200 -and $_.result.url -eq 'http://fixture:8081/snapshot'});Assert-Scenario ($fetches.Count -ge 1 -and ($fetches[-1].result.content|ConvertFrom-Json).authenticated) 'Report did not use an actual broker-authenticated API result.';$refs=@($t.work_orders|ForEach-Object {$_.contract.evidence});Assert-Scenario (@($refs|Where-Object {$Content.Contains($_.resolved.reference.item_id)}).Count -ge 1 -and $Content -match 'v[1-9]|version|버전') 'Report omitted pinned source IDs or version citations.'}
Invoke-Compose @('--profile','verify-api','up','-d','--no-build','fixture')
if($Resume){foreach($name in @('rule','failure','codeRule','routing','inventory','secret','reportContract','reportGoal')){Set-Variable -Name $name -Value $progress.vars[$name]}}else{
$source=@{category='data';title='가상 주간 운영 API와 보고서 규칙';storage_mode='excerpt';sources=@(@{uri='https://example.com/echelon-owned/report-policy';note='Owned fixture, do not fetch this reference'});content='For this synthetic fixture, read the permitted http://fixture:8081/snapshot API with the assigned opaque secret_ref. It returns requests=120, resolved=108, pending=12, success_rate=0.9. Write report.md with a table, 90% resolution rate, unresolved limitations, and pinned item/version citations. This procedure is reusable for the owned local fixture only.';tags=@('검증:주간보고');truth_state='supported';applicability='Owned local fixture only; never treat these values as real company data'}
$rule=Scenario-Knowledge $source
$failure=Scenario-Knowledge @{category='info';title='가상 보고서의 오래된 80% 값은 재사용 금지';storage_mode='excerpt';content='Historical 80% resolution assumption was false and failed verification. Use the actual API value and cite the failure as excluded history, not as the current result.';relations=@(@{kind='derived_from';target_id=$rule.submission.item_id;target_version=1});tags=@('검증:주간보고');truth_state='refuted';outcome='failed';assessment=@{conditions='Owned synthetic stale report';attempted_action='Copy an assumed 80%';observed='Actual API reports 90%';reason='Stale assumption, no current source';avoid='Reusing assumed 80%';retry_when='Independently verified new source'}}
$codeRule=Scenario-Knowledge @{category='info';title='격리 코드 변경·검증의 범위';storage_mode='excerpt';content='For tag 검증:코드변경, read the provided rates.py and test_rates.py inputs. Change only files in the ephemeral sandbox, run the supplied unit tests, recover the changed source and verification output as artifacts. Never claim a deployment or host repository change.';sources=@(@{uri='https://example.com/echelon-owned/code-policy'});tags=@('검증:코드변경');truth_state='supported'}
$routing=Scenario-Knowledge @{category='info';title='가상 업무 요청 분류 기준';storage_mode='excerpt';content='Owned requests: invoice -> Finance/P2; password reset -> IT/P2; production outage -> Platform/P1. Write routing.csv with columns id,department,priority and one row per input. Ambiguous requests must be marked NeedsReview/P2. These labels apply only to the synthetic input.';sources=@(@{uri='https://example.com/echelon-owned/routing-policy'});tags=@('검증:업무분류');truth_state='supported'}
$inventory=@($rule,$failure,$codeRule,$routing)
foreach($n in 1..3){$inventory+=Scenario-Knowledge @{category='data';title="다른 업무의 가상 참고 노트 $n";storage_mode='excerpt';sources=@(@{uri="https://example.com/echelon-owned/unrelated-$n"});content=(('This owned reference describes a separate inventory process, naming conventions, retention conditions and source checks. It has no weekly report evidence. '*20));tags=@('검증:다른업무');truth_state='unverified'}}
$secret=Invoke-Echelon '/v1/operator/secrets' 'POST' @{tenant_id=$tenant;name='가상 운영 API 인증';purpose='Owned read-only report fixture';allowed_urls=@('http://fixture:8081/snapshot');value=('Bearer '+(New-RuntimeSecret))}
$reportContract=@{kind='work';output_format='report.md';completion_conditions=@('Read actual permitted API','120 total,108 resolved,12 pending,90% current rate','Exclude historical 80% as refuted failure','Recover report.md with pinned citations');constraints=@('Owned synthetic fixture only','No external writes or deployment');allowed_tools=@('external.fetch','artifact.write');urls=@(@{url='http://fixture:8081/snapshot';allow_private=$true;allow_http=$true});secret_refs=@($secret.secret_ref);input_artifacts=@();verification='Validate report artifact against fixture values and pinned failure evidence';max_work_orders=2}
$reportGoal="가상 주간 운영 보고서 작성. 중앙 지식의 태그 검증:주간보고 근거를 사용하고 http://fixture:8081/snapshot 을 실제로 읽으세요. 해당 Work Order의 secret_ref를 사용하고 report.md를 산출하세요. 120건 중108건 해결,12건 대기,현재90%를 표로 쓰고 과거80%가 반증된 실패 이력임을 구분하세요. 중요 근거에는 정보ID와버전을 인용하고 실제 회사 데이터가 아닌 검증용 자료임을 명시하세요."
$progress.vars=@{rule=$rule;failure=$failure;codeRule=$codeRule;routing=$routing;inventory=$inventory;secret=$secret;reportContract=$reportContract;reportGoal=$reportGoal};Save-ScenarioProgress
}
$baselineTask=Scenario-StageTask 'baseline' $reportGoal $reportContract
if($progress.pack){$pack=@{pack=@{id=$progress.pack}}}else{
$request=Scenario-Domain $baselineTask 'context.request' @{request_id=[guid]::NewGuid().ToString();criteria=@{need='Controlled Handler/Agent baseline with the complete owned inventory';tags=@('검증:주간보고')}}
$selections=@($inventory|ForEach-Object {@{item_id=$_.submission.item_id;version=$_.submission.version;reason='Operator-prepared full-context baseline'}})
$pack=Scenario-Domain $admin 'context.curate' @{op='deliver';request_id=$request.request.id;selections=$selections;gaps=@()}
$progress.pack=$pack.pack.id;Save-ScenarioProgress
}
$baseline=Scenario-Run $baselineTask @{prepared_pack_id=$pack.pack.id;eager_context=$true}
$b=Scenario-Output $baselineTask 'report.md'
Check-ScenarioReport $baselineTask $b.content
foreach($v in @('120','108','12','90','80')){Assert-Scenario ($b.content.Contains($v)) "Baseline report omitted required value $v"}
Assert-Scenario (@($baseline.events|Where-Object {$_.kind -eq 'model.completed' -and $_.role -eq 'analyst'}).Count -eq 0) 'The Handler/Agent baseline unexpectedly called Analyst.'
$analystTask=Scenario-StageTask 'analyst' $reportGoal $reportContract
$analystRun=Scenario-Run $analystTask
$a=Scenario-Output $analystTask 'report.md'
Check-ScenarioReport $analystTask $a.content
foreach($v in @('120','108','12','90','80')){Assert-Scenario ($a.content.Contains($v)) "Analyst report omitted required value $v"}
Assert-Scenario (@($analystRun.events|Where-Object {$_.kind -eq 'model.completed' -and $_.role -eq 'analyst'}).Count -gt 0) 'Analyst selection did not use the actual model.'
$masterTask=Scenario-StageTask 'master' "Explicitly create one manual reusable protocol, codename 알파. Describe the owned weekly report procedure from existing tag 검증:주간보고 records. Include current source and false/failure history, no unrelated knowledge. Set parameters:[], status:candidate, source_tasks:[$baselineTask,$analystTask]. Inspect records and audit, then submit a protocol proposal; do not execute business work. This is an explicitly requested manual protocol even before three repetitions." @{kind='dream'}
$master=Scenario-Run $masterTask @{mode='dream'}
Assert-Scenario (@($master.events|Where-Object {$_.role -eq 'master' -and $_.kind -eq 'model.completed'}).Count -gt 0) 'No actual Master model execution.'
if(-not $progress.protocol){
$proposals=@((Invoke-Echelon "/v1/operator/maintenance?tenant_id=$tenant").proposals|Where-Object {$_.task_id -eq $masterTask -and $_.kind -eq 'protocol' -and $_.status -eq 'pending'})
Assert-Scenario ($proposals.Count -eq 1) 'The explicit manual protocol was not proposed exactly once.'
$applied=Invoke-Echelon '/v1/operator/maintenance/decide' 'POST' @{tenant_id=$tenant;proposal_id=$proposals[0].id;accept=$true;reason='Reviewed owned synthetic protocol against its existing report evidence';activate_protocol=$true}
$protocol=$applied.decision.changes[0].protocol_id
$progress.protocol=$protocol;Save-ScenarioProgress
}else{$protocol=$progress.protocol}
$loaded=Invoke-Echelon "/v1/operator/protocols/$protocol`?tenant_id=$tenant"
Assert-Scenario ($loaded.current_status -eq 'active') 'Reviewed protocol was not activated.'
Assert-Scenario ((Invoke-Echelon "/v1/operator/protocols/$protocol`?tenant_id=$tenant").cache_hit) 'Protocol information-set cache did not hit.'
$protocolTask=Scenario-StageTask 'protocol' $reportGoal $reportContract
$protocolRun=Scenario-Run $protocolTask @{protocol_id=$protocol;protocol_version=$loaded.version;parameters=@{}}
$p=Scenario-Output $protocolTask 'report.md'
Check-ScenarioReport $protocolTask $p.content
foreach($v in @('120','108','12','90','80')){Assert-Scenario ($p.content.Contains($v)) "Protocol report omitted required value $v"}
Assert-Scenario (@($protocolRun.events|Where-Object {$_.kind -eq 'model.completed' -and $_.role -eq 'analyst'}).Count -eq 0) 'Protocol reuse performed a fresh Analyst model search.'
Assert-Scenario (@($protocolRun.events|Where-Object kind -eq 'protocol.direct_load').Count -gt 0) 'Protocol direct information-set loading did not occur.'
$protocolOrders=@((Invoke-Echelon "/v1/operator/tasks/$protocolTask`?tenant_id=$tenant").work_orders)
Assert-Scenario ($protocolOrders.Count -gt 0) 'Protocol execution has no Work Order.'
foreach($order in $protocolOrders){Assert-Scenario ($order.contract.protocol.id -eq $protocol -and $order.contract.protocol.version -eq $loaded.version -and @($order.contract.protocol.steps).Count -gt 0) 'Protocol procedure was not pinned in the actual Work Order.'}
$repetitions=(Scenario-Domain $admin 'knowledge.optimize' @{op='inspect'}).repetitions
Assert-Scenario (@($repetitions|Where-Object {$_.completed_tasks -ge 3 -and $_.task_ids -contains $baselineTask -and $_.task_ids -contains $analystTask -and $_.task_ids -contains $protocolTask}).Count -eq 1) 'Three completed report Tasks were not recognized as a repeated work structure.'
$codeContract=@{kind='work';output_format='rates.py and verification.txt';allowed_tools=@('shell.exec');max_work_orders=2;completion_conditions=@('All supplied unittest cases pass','Return changed rates.py and verification.txt');verification='python3 -m unittest -v test_rates.py';constraints=@('Ephemeral sandbox only; no deployment')}
$codeTask=Scenario-StageTask 'code' '검증:코드변경. 제공한 rates.py의 resolved_rate를 고치세요. 0건 중0건은0.0%, 음수 또는해결건수>전체건수는ValueError, 그외백분율은소수첫째자리까지반올림합니다. test_rates.py를변경하지말고 python3 -m unittest -v test_rates.py를실제로실행하여 전부통과시켜야합니다. rates.py와검증로그verification.txt를산출물로회수하세요. 완료후이테스트로확인된재사용가능한경계조건교훈을artifact출처로Handler가지식제출하고Analyst가검토하도록하세요.' $codeContract
$existingCode=Invoke-Echelon "/v1/operator/tasks/$codeTask`?tenant_id=$tenant"
if(-not @($existingCode.contract.input_artifacts).Count){
$rates=Scenario-Upload $codeTask 'rates.py' "def resolved_rate(resolved, total):`n    return resolved / total * 100`n"
$tests=@'
import unittest
from rates import resolved_rate
class RatesTests(unittest.TestCase):
 def test_normal(self): self.assertEqual(resolved_rate(108,120),90.0)
 def test_empty(self): self.assertEqual(resolved_rate(0,0),0.0)
 def test_round(self): self.assertEqual(resolved_rate(1,3),33.3)
 def test_invalid(self):
  for pair in [(-1,2),(2,-1),(3,2),(1,0)]:
   with self.assertRaises(ValueError): resolved_rate(*pair)
if __name__=='__main__': unittest.main()
'@
$testFile=Scenario-Upload $codeTask 'test_rates.py' $tests
$codeContract.input_artifacts=@($rates,$testFile)
$null=Invoke-Echelon "/v1/operator/tasks/$codeTask" 'PATCH' @{tenant_id=$tenant;contract=$codeContract;reason='Grant the two owned code and test inputs'}
}
$codeRun=Scenario-Run $codeTask @{retain_knowledge=$true;dream_after=$true}
$verification=Scenario-Output $codeTask 'verification.txt'
Assert-Scenario ($verification.content -match 'Ran 4 tests' -and $verification.content -match '\bOK\b') 'Actual recovered unit-test output did not pass all four tests.'
$changedCode=Scenario-Output $codeTask 'rates.py'
Assert-Scenario ($changedCode.content -match 'ValueError') 'Recovered source does not implement the required invalid-input gate.'
$independentContract=@{kind='work';output_format='independent-check.txt';allowed_tools=@('shell.exec');max_work_orders=1;verification='Operator-controlled original unittest suite against recovered source'}
$independentTask=Scenario-StageTask 'independent_code' ("Independent native verification of recovered rates.py artifact "+$changedCode.artifact.id) $independentContract
$independentState=Invoke-Echelon "/v1/operator/tasks/$independentTask`?tenant_id=$tenant"
if(-not @($independentState.artifacts|Where-Object {$_.work_order_id -and $_.name -eq 'independent-check.txt'}).Count){
 $originalTask=Invoke-Echelon "/v1/operator/tasks/$codeTask`?tenant_id=$tenant";$originalTests=@($originalTask.artifacts|Where-Object {$_.name -eq 'test_rates.py' -and -not $_.work_order_id});Assert-Scenario ($originalTests.Count -eq 1) 'Original operator test fixture is ambiguous.'
 $originalTest=Invoke-Echelon "/v1/operator/artifacts/$($originalTests[0].id)?tenant_id=$tenant"
 $copiedSource=Scenario-Upload $independentTask 'rates.py' $changedCode.content;$copiedTests=Scenario-Upload $independentTask 'test_rates.py' $originalTest.content
 $independentContract.input_artifacts=@($copiedSource,$copiedTests)
 $null=Invoke-Echelon "/v1/operator/tasks/$independentTask" 'PATCH' @{tenant_id=$tenant;contract=$independentContract;reason='Explicitly grant recovered source and immutable operator-authored tests'}
 $null=Scenario-Domain $independentTask 'task.plan' @{steps=@('Run the original fixed tests against the actual recovered source')}
 $orderKey=[guid]::new([byte[]]([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($independentTask+':independent-order')))[0..15])
 $checkOrder=Scenario-Domain $independentTask 'agent.delegate' @{request_id=$orderKey.ToString();target='owned:independent-code';objective='Run the fixed operator test suite';expected_output='independent-check.txt'}
 $checkAgent=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$independentTask;work_order_id=$checkOrder.work_order_id;targets=@('owned:independent-code')}).token
 $checked=Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$independentTask;tool='shell.exec';target='owned:independent-code';input=@{request_id=$orderKey.ToString();script='python3 -m unittest -v test_rates.py > independent-check.txt 2>&1';outputs=@('independent-check.txt');timeout_seconds=10}} $checkAgent
 Assert-Scenario ($checked.exit_code -eq 0 -and -not $checked.timed_out) 'Independent fixed tests failed against the recovered source.'
 $null=Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$independentTask;tool='work_order.complete';input=@{outcome='completed';summary='Operator-controlled independent tests passed';evidence=@();limitations=@();artifacts=@($checked.artifacts[0].id)}} $checkAgent
 $checkAgent=$null;$null=Invoke-Echelon "/v1/operator/tasks/$independentTask" 'PATCH' @{tenant_id=$tenant;status='completed';reason='Independent native verification receipt passed'}
}
$independentOutput=Scenario-Output $independentTask 'independent-check.txt'
Assert-Scenario ($independentOutput.content -match 'Ran 4 tests' -and $independentOutput.content -match '\bOK\b') 'Independent recovered-code check did not pass all four original tests.'
foreach($role in @('analyst','handler','agent','master')){Assert-Scenario (@($codeRun.events|Where-Object {$_.role -eq $role -and $_.kind -eq 'model.completed'}).Count -gt 0) "No actual model execution for $role in the complete work loop."}
foreach($tool in @('knowledge.submit','knowledge.publish','run.adoption_done','knowledge.optimize','run.dream_done')){Assert-Scenario (@($codeRun.events|Where-Object {$_.kind -eq 'tool.completed' -and $_.details.tool -eq $tool}).Count -gt 0) "Missing actual post-processing effect: $tool"}
$classContract=@{kind='work';output_format='routing.csv';allowed_tools=@('shell.exec','artifact.write');max_work_orders=2;completion_conditions=@('One row per request','Apply registered routing policy','Recover routing.csv');verification='Check exact IDs, departments and priorities in the recovered CSV'}
$classTask=Scenario-StageTask 'classification' '검증:업무분류 태그의 중앙기준을사용해제공한requests.json을분류하세요. routing.csv에id,department,priority열과REQ001,REQ002,REQ003,REQ004각행을만드세요. 모호한요청은NeedsReview/P2로남깁니다. 실제파일을읽고완성한CSV를산출물로회수하고각판단의고정근거를결과에인용하세요. 메일발송이나외부변경은하지않습니다.' $classContract
$existingClass=Invoke-Echelon "/v1/operator/tasks/$classTask`?tenant_id=$tenant"
if(-not @($existingClass.contract.input_artifacts).Count){
$requests=Scenario-Upload $classTask 'requests.json' (@(@{id='REQ001';request='invoice approval'},@{id='REQ002';request='password reset'},@{id='REQ003';request='production outage'},@{id='REQ004';request='Please handle this thing'})|ConvertTo-Json -Compress)
$classContract.input_artifacts=@($requests)
$null=Invoke-Echelon "/v1/operator/tasks/$classTask" 'PATCH' @{tenant_id=$tenant;contract=$classContract;reason='Grant the owned request fixture'}
}
$classRun=Scenario-Run $classTask @{retain_knowledge=$true}
$csv=Scenario-Output $classTask 'routing.csv';$rows=@($csv.content|ConvertFrom-Csv)
Assert-Scenario ($rows.Count -eq 4) 'Classification output row count differs from the input.'
foreach($expected in @(@('REQ001','Finance','P2'),@('REQ002','IT','P2'),@('REQ003','Platform','P1'),@('REQ004','NeedsReview','P2'))){$row=@($rows|Where-Object id -eq $expected[0]);Assert-Scenario ($row.Count -eq 1 -and $row[0].department -eq $expected[1] -and $row[0].priority -eq $expected[2]) "Wrong classification for $($expected[0])"}
function Usage-Scenario($Run){$events=@($Run.events|Where-Object kind -eq 'model.completed');@{run_id=$Run.id;calls=$Run.calls;input_tokens=($events|ForEach-Object {[long]$_.details.usage.input_tokens}|Measure-Object -Sum).Sum;output_tokens=($events|ForEach-Object {[long]$_.details.usage.output_tokens}|Measure-Object -Sum).Sum;elapsed_ms=$Run.elapsed_ms;quality='required values and actual artifacts verified'}}
$usage=@{baseline=Usage-Scenario $baseline;analyst=Usage-Scenario $analystRun;protocol=Usage-Scenario $protocolRun;protocol_setup_and_audit=Usage-Scenario $master}
$usage.protocol_input_saving_pct=[Math]::Round((1-$usage.protocol.input_tokens/$usage.baseline.input_tokens)*100,2)
$usage.analyst_input_saving_pct=[Math]::Round((1-$usage.analyst.input_tokens/$usage.baseline.input_tokens)*100,2)
$usage.setup_amortized_input_tokens=@{one_reuse=$usage.protocol.input_tokens+$usage.protocol_setup_and_audit.input_tokens;five_reuses=$usage.protocol.input_tokens+$usage.protocol_setup_and_audit.input_tokens/5;ten_reuses=$usage.protocol.input_tokens+$usage.protocol_setup_and_audit.input_tokens/10}
$usage.note='Single controlled synthetic sample per profile; same model, goal, input and completion criteria. Baseline receives the full operator-approved inventory. Work retention is disabled equally for the three report profiles. Master protocol setup/audit cost is listed separately and amortized. Code and classification include their actual retention/audit costs in their run records. No production savings or statistical significance claim.'
$result=@{passed=$true;assertions=$script:ScenarioAssertions;tenant_id=$tenant;model=$Model;reports=@($baselineTask,$analystTask,$protocolTask);code_task=$codeTask;independent_code_check=$independentTask;classification_task=$classTask;master_task=$masterTask;protocol_id=$protocol;usage=$usage;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/work-scenarios.json') ($result|ConvertTo-Json -Depth 16)
$result|ConvertTo-Json -Depth 16 -Compress
