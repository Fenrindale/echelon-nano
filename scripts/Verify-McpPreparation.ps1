param([switch]$WithOAuth,[switch]$OAuthOnly,[switch]$Resume)
. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'MCP preparation verification requires the isolated port 18432 copy.'}
$script:PreparationChecks=0
function Check-Preparation([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:PreparationChecks++}
function Preparation-Domain([string]$Task,[string]$Tool,$Data){Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$Task;tool=$Tool;input=$Data}}
function Preparation-Token([string]$Role,[string]$Task,[string]$Order=''){
 $request=@{role=$Role;tenant_id=$tenant;task_id=$Task}
 if($Order){$request.work_order_id=$Order;$request.targets=@('owned:prepare')}
 (Invoke-Echelon '/v1/operator/role-tokens' 'POST' $request).token
}
function Preparation-Call([string]$Tool,$Data,[string]$Token=$handler,[string]$Task=$task){Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$Task;tool=$Tool;target='owned:prepare';input=$Data} $Token}
function Deny-Preparation([string]$Tool,$Data,[string]$Token=$handler,[int]$Expected=403){
 $response=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization=('Bearer '+$Token)} -ContentType 'application/json' -Body (@{tenant_id=$tenant;task_id=$task;tool=$Tool;input=$Data}|ConvertTo-Json -Depth 25 -Compress) -SkipHttpErrorCheck
 Check-Preparation ([int]$response.StatusCode -eq $Expected) ('Unexpected denial for '+$Tool+': '+$response.StatusCode)
}
function Preparation-FixtureState {
 $code='import json,urllib.request; print(json.dumps(json.load(urllib.request.urlopen("http://127.0.0.1:8081/mcp-state"))))'
 $out=docker compose exec -T fixture python3 -c $code
 if($LASTEXITCODE -ne 0){throw 'Fixture state query failed'}
 $out|ConvertFrom-Json -AsHashtable
}
function Preparation-Count($State,[string]$Key){if($State.counts.ContainsKey($Key)){return [int]$State.counts[$Key]};0}
Push-Location $script:ProjectRoot
try {
if(-not $OAuthOnly -and -not $Resume){
 $active=docker compose exec -T postgres psql -X -qAt -U echelon -d echelon -c "SELECT count(*) FROM execution_runs WHERE status IN ('queued','running')"
 if($LASTEXITCODE -ne 0 -or [int]("$active".Trim()) -ne 0){throw 'Stop other isolated inference before the controlled core restart'}
 Invoke-Compose @('--profile','verify-api','up','-d','--no-build','fixture')
 $tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned Handler MCP preparation boundaries'}).id
 $source=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned Handler MCP preparation boundaries';goal='Import owned preparation fixture credentials';contract=@{kind='work';allowed_tools=@('credential.import')}}).task_id
 $inputFile=(Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$source;artifact=@{request_id=[guid]::NewGuid().ToString();name='owned-prepare.env';content="MCP_TOKEN=owned-mcp-prepare-value`n"}}).artifact.id
 $sourceContract=@{kind='work';allowed_tools=@('credential.import');input_artifacts=@($inputFile)}
 $null=Invoke-Echelon ('/v1/operator/tasks/'+$source) 'PATCH' @{tenant_id=$tenant;contract=$sourceContract;reason='Owned preparation fixture input grant'}
 $importOrder=Preparation-Domain $source 'agent.delegate' @{request_id=[guid]::NewGuid().ToString();target='owned:prepare';objective='Import synthetic credential';expected_output='Storage receipt'}
 $importAgent=Preparation-Token 'agent' $source $importOrder.work_order_id
 $bundle=(Preparation-Call 'credential.import' @{artifact_id=$inputFile;request_id=[guid]::NewGuid().ToString()} $importAgent $source).bundle.id
 $goal='Owned preparation fixture at http://fixture:8081/mcp and http://fixture:8081/not-mcp only. Resolve owned synthetic work items after preparation; no external business data.'
 $contract=@{kind='work';allowed_tools=@('mcp.call');max_work_orders=3}
 $task=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned Handler MCP preparation boundaries';goal=$goal;contract=$contract}).task_id
 $handler=Preparation-Token 'handler' $task
 $search=Preparation-Call 'mcp.search' @{query='@modelcontextprotocol/server-everything';transport='npm';limit=8}
 Check-Preparation (@($search.packages|Where-Object package -eq '@modelcontextprotocol/server-everything').Count -eq 1) 'Live npm registry search did not return the exact official package'
 $description=Preparation-Call 'mcp.describe' @{package='@modelcontextprotocol/server-everything';transport='npm'}
 Check-Preparation ($description.untrusted_metadata -and $description.suggested_spec.executable -eq 'mcp-server-everything' -and $description.suggested_spec.package_version -match '^\d') 'Live npm metadata did not pin a usable release and entrypoint'
 $python=Preparation-Call 'mcp.describe' @{package='mcp-server-time';transport='python'}
 Check-Preparation ($python.untrusted_metadata -and $python.suggested_spec.transport -eq 'python' -and $python.python_module_is_inferred) 'Live PyPI metadata lookup did not retain inferred-module provenance'
 $pythonReady=Preparation-Call 'mcp.prepare' @{request_id=[guid]::NewGuid().ToString();name='Owned prepared Python clock';reason='Verify an actual wheel-installed Python MCP in the owned fixture';spec=$python.suggested_spec}
 Check-Preparation ($pythonReady.status -eq 'ready' -and @($pythonReady.tools|Where-Object name -eq 'get_current_time').Count -eq 1) 'Handler did not install and initialize the Python MCP'
 $spec=@{transport='http';url='http://fixture:8081/mcp';allow_private=$true;allow_http=$true;environment=@(@{name='MCP_AUTH_TOKEN';bundle_id=$bundle;variable='MCP_TOKEN'})}
 $request=@{request_id=[guid]::NewGuid().ToString();name='Owned Handler prepared MCP';reason='Required for the owned Task';spec=$spec}
 Deny-Preparation 'mcp.prepare' $request
 $contract.credential_bundles=@($bundle)
 $null=Invoke-Echelon ('/v1/operator/tasks/'+$task) 'PATCH' @{tenant_id=$tenant;contract=$contract;reason='Grant the existing synthetic credential bundle before preparation'}
 foreach($role in @('analyst','master')){
  $token=Preparation-Token $role $task
  foreach($tool in @('mcp.search','mcp.describe','mcp.prepare')){Deny-Preparation $tool @{} $token}
 }
 $sourceContract.model_access='blocked'
 $null=Invoke-Echelon ('/v1/operator/tasks/'+$source) 'PATCH' @{tenant_id=$tenant;contract=$sourceContract;reason='Owned restricted source check'}
 Deny-Preparation 'mcp.prepare' $request
 $sourceContract.model_access=''
 $null=Invoke-Echelon ('/v1/operator/tasks/'+$source) 'PATCH' @{tenant_id=$tenant;contract=$sourceContract;reason='Restore exact source policy after owned denial'}
 $before=Preparation-FixtureState
 $ready=Preparation-Call 'mcp.prepare' $request
 Check-Preparation ($ready.status -eq 'ready' -and $ready.version -eq 1 -and @($ready.tools|Where-Object name -eq 'resolve_item').Count -eq 1) 'Handler did not prepare/discover actual MCP tools'
 Check-Preparation (($ready|ConvertTo-Json -Depth 20) -notmatch 'owned-mcp-prepare-value') 'Preparation exposed a Vault value'
 $after=Preparation-FixtureState
 Check-Preparation ((Preparation-Count $before 'mcp:tools/call') -eq (Preparation-Count $after 'mcp:tools/call')) 'Preparation called a business tool'
 $replayed=Preparation-Call 'mcp.prepare' $request
 Check-Preparation (($replayed|ConvertTo-Json -Depth 20 -Compress) -eq ($ready|ConvertTo-Json -Depth 20 -Compress)) 'Preparation replay did not return its immutable receipt'
 $replayedState=Preparation-FixtureState
 Check-Preparation ((Preparation-Count $after 'mcp:initialize') -eq (Preparation-Count $replayedState 'mcp:initialize')) 'Replay repeated initialization'
 $different=$request.Clone();$different.name='Different meaning for the same request'
 Deny-Preparation 'mcp.prepare' $different $handler 400
 $again=$request.Clone();$again.request_id=[guid]::NewGuid().ToString()
 $reused=Preparation-Call 'mcp.prepare' $again
 Check-Preparation ($reused.id -eq $ready.id -and $reused.reused) 'Identical workspace configuration was duplicated'
 # Simulate a controller interruption after schema persistence but before its
 # immutable preparation result was committed. Reconciliation must not initialize again.
 $pending=$request.Clone();$pending.request_id=[guid]::NewGuid().ToString()
 $sql="INSERT INTO mcp_preparations(tenant_id,task_id,request_key,request_hash,request,server_id,server_version,reused) SELECT tenant_id,task_id,'$($pending.request_id)',request_hash,request,server_id,server_version,reused FROM mcp_preparations WHERE tenant_id='$tenant' AND task_id='$task' AND request_key='$($request.request_id)';"
 $null=$sql|docker compose exec -T postgres psql -X -qAt -v ON_ERROR_STOP=1 -U echelon -d echelon
 if($LASTEXITCODE -ne 0){throw 'Owned interruption fixture creation failed'}
 $pendingState=Preparation-FixtureState
 $reconciled=Preparation-Call 'mcp.prepare' $pending
 Check-Preparation ($reconciled.status -eq 'ready' -and $reconciled.id -eq $ready.id) 'Pending preparation could not reconcile its durable schema'
 $reconciledState=Preparation-FixtureState
 Check-Preparation ((Preparation-Count $pendingState 'mcp:initialize') -eq (Preparation-Count $reconciledState 'mcp:initialize')) 'Reconciliation repeated initialization'
 $changedSpec=$spec.Clone();$changedSpec.environment=@($spec.environment)+@(@{name='EXTRA_CONFIG';bundle_id=$bundle;variable='MCP_TOKEN'})
 $changed=Preparation-Call 'mcp.prepare' @{request_id=[guid]::NewGuid().ToString();name='Owned Handler prepared MCP';reason='Create a distinct configuration preserving prior grants';spec=$changedSpec}
 Check-Preparation ($changed.status -eq 'ready' -and $changed.id -ne $ready.id) 'Changed configuration overwrote the existing server'
 $badSpec=$spec.Clone();$badSpec.url='http://fixture:8081/not-mcp'
 $failedRequest=@{request_id=[guid]::NewGuid().ToString();name='Owned failed preparation';reason='Retain a real failed initialization';spec=$badSpec}
 $failed=Preparation-Call 'mcp.prepare' $failedRequest
 Check-Preparation ($failed.status -eq 'failed' -and $failed.error_code) 'Failed initialization was presented as ready'
 $failedReplay=Preparation-Call 'mcp.prepare' $failedRequest
 Check-Preparation (($failedReplay|ConvertTo-Json -Depth 20 -Compress) -eq ($failed|ConvertTo-Json -Depth 20 -Compress)) 'A failed preparation was silently rerun or rewritten'
 $unsafe=$spec.Clone();$unsafe.url='http://vault:8200/v1/sys/health'
 Deny-Preparation 'mcp.prepare' @{request_id=[guid]::NewGuid().ToString();name='Denied endpoint';reason='Owned negative test';spec=$unsafe}
 $unsafe=$spec.Clone();$unsafe.environment=@(@{name='PATH';bundle_id=$bundle;variable='MCP_TOKEN'})
 Deny-Preparation 'mcp.prepare' @{request_id=[guid]::NewGuid().ToString();name='Denied environment';reason='Owned negative test';spec=$unsafe} $handler 400
 $other=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned Handler MCP preparation boundaries';goal=$goal;contract=@{kind='work';allowed_tools=@('mcp.call')}}).task_id
 $otherHandler=Preparation-Token 'handler' $other
 $denied=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization=('Bearer '+$otherHandler)} -ContentType 'application/json' -Body (@{tenant_id=$tenant;task_id=$other;tool='mcp.prepare';input=$request}|ConvertTo-Json -Depth 20 -Compress) -SkipHttpErrorCheck
 Check-Preparation ([int]$denied.StatusCode -eq 403) 'Another Task reused a credential without a Task grant'
 Invoke-Compose @('restart','core')
 $deadline=[DateTimeOffset]::UtcNow.AddSeconds(45)
 do {try{$health=Invoke-RestMethod ($script:ApiBase+'/readyz') -TimeoutSec 2;if($health.status -eq 'ready'){break}}catch{};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Core restart recovery failed'};Start-Sleep -Milliseconds 500}while($true)
 $afterRestart=Preparation-Call 'mcp.prepare' $request
 Check-Preparation (($afterRestart|ConvertTo-Json -Depth 20 -Compress) -eq ($ready|ConvertTo-Json -Depth 20 -Compress)) 'Preparation receipt did not survive restart'
 $contract.mcp_tools=@(@{server_id=$ready.id;version=$ready.version;tools=@('list_items','resolve_item')},@{server_id=$pythonReady.id;version=$pythonReady.version;tools=@('get_current_time')})
 $null=Invoke-Echelon ('/v1/operator/tasks/'+$task) 'PATCH' @{tenant_id=$tenant;contract=$contract;reason='Pin Handler prepared tools before Agent delegation'}
 $order=Preparation-Domain $task 'agent.delegate' @{request_id=[guid]::NewGuid().ToString();target='owned:prepare';objective='Use Handler prepared tools';expected_output='Actual scoped effect receipts'}
 $agent=Preparation-Token 'agent' $task $order.work_order_id
 foreach($tool in @('mcp.search','mcp.describe','mcp.prepare')){Deny-Preparation $tool @{} $agent}
 Deny-Preparation 'mcp.call' @{server_id=$ready.id;tool='list_items';arguments=@{};request_id=[guid]::NewGuid().ToString()} $handler
 $read=Preparation-Call 'work_order.read' @{work_order_id=$order.work_order_id} $agent
 Check-Preparation ($read.contract.mcp_schemas[0].server_id -eq $ready.id) 'Prepared schema was not pinned in the actual Work Order'
 $list=Preparation-Call 'mcp.call' @{request_id=[guid]::NewGuid().ToString();server_id=$ready.id;tool='list_items';arguments=@{}} $agent
 $content=$list.result.content[0].text|ConvertFrom-Json
 Check-Preparation ($content.authenticated -and ($list|ConvertTo-Json -Depth 20) -notmatch 'owned-mcp-prepare-value') 'Agent call did not receive a broker-injected masked credential'
 $effect=Preparation-Call 'mcp.call' @{request_id=[guid]::NewGuid().ToString();server_id=$ready.id;tool='resolve_item';arguments=@{id='owned-prepare-item'}} $agent
 Check-Preparation (($effect.result.content[0].text|ConvertFrom-Json).resolved -contains 'owned-prepare-item') 'Actual Agent business effect was not observed'
 $time=Preparation-Call 'mcp.call' @{request_id=[guid]::NewGuid().ToString();server_id=$pythonReady.id;tool='get_current_time';arguments=@{timezone='UTC'}} $agent
 Check-Preparation ($time.operation_ok -and $time.result.content[0].text -match 'UTC') 'Agent could not execute the Handler prepared Python MCP'
 $native=@{passed=$true;assertions=$script:PreparationChecks;handler_prepared=$true;no_business_call_during_preparation=$true;immutable_replay=$true;restart_replay=$true;changed_config_preserves_prior=$true;failed_attempt_preserved=$true;config_reused=$true;vault_masked=$true;agent_effect_verified=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
 Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/mcp-preparation-native.json') ($native|ConvertTo-Json)
 $native|ConvertTo-Json -Compress
}
if($WithOAuth -or $OAuthOnly -or $Resume){
 $progressPath=Join-Path $script:RuntimeRoot 'evidence/mcp-preparation-active.json'
 if($Resume){$progress=Get-Content $progressPath -Raw|ConvertFrom-Json -AsHashtable;$run=Invoke-Echelon ('/v1/operator/runs/'+$progress.run_id);if($run.status -in @('paused','interrupted','failed')){$null=Invoke-Echelon ('/v1/operator/runs/'+$progress.run_id+'/control') 'POST' @{action='resume'}}}
 else {
  $tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned automatic MCP preparation OAuth'}).id
  $launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal='Owned synthetic MCP preparation check. The workspace initially has NO MCP server. Prepare a reusable npm stdio MCP using the official @modelcontextprotocol/server-everything package. Handler must look up published registry metadata with mcp.search or mcp.describe, choose a real pinned version and executable, install/initialize it using mcp.prepare, and grant ONLY echo to Agent. Agent must actually call echo with message MCP_PREPARED_73, write its returned text into mcp-prepared.txt and read the file back. Completion requires the actual MCP effect receipt and exact recovered file content MCP_PREPARED_73. This is a synthetic local echo; no credentials, private endpoint or business change is needed. Handler configures everything without requesting technical user input. Central knowledge creation and Master audit are unnecessary for this fixture.'}
  $progress=@{tenant_id=$tenant;task_id=$launch.task_id;run_id=$launch.run_id}
  Write-PrivateText $progressPath ($progress|ConvertTo-Json)
 }
 $deadline=[DateTimeOffset]::UtcNow.AddMinutes(20)
 do{$run=Invoke-Echelon ('/v1/operator/runs/'+$progress.run_id);Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/mcp-preparation-oauth-run.json') ($run|ConvertTo-Json -Depth 35);if($run.status -notin @('queued','running')){break};if([DateTimeOffset]::UtcNow -gt $deadline){throw 'Automatic MCP preparation timed out'};Start-Sleep -Seconds 2}while($true)
 Check-Preparation ($run.status -eq 'completed' -and $run.result.outcome -eq 'completed') ('Automatic MCP preparation stopped: '+$run.status+' / '+$run.error_code+' / '+$run.result.outcome)
 $effects=@($run.events|Where-Object kind -eq 'tool.completed')
 Check-Preparation (@($effects|Where-Object {$_.role -eq 'handler' -and $_.details.tool -in @('mcp.search','mcp.describe')}).Count -ge 1) 'Handler did not use registry discovery'
 Check-Preparation (@($effects|Where-Object {$_.role -eq 'handler' -and $_.details.tool -eq 'mcp.prepare'}).Count -ge 1) 'Handler did not prepare the MCP itself'
 Check-Preparation (@($effects|Where-Object {$_.role -eq 'agent' -and $_.details.tool -eq 'mcp.call'}).Count -ge 1) 'Agent did not execute the prepared MCP tool'
 Check-Preparation (@($effects|Where-Object {$_.details.tool -eq 'run.request_input'}).Count -eq 0) 'Automatic preparation requested technical user input'
 $taskDetail=Invoke-Echelon ('/v1/operator/tasks/'+$progress.task_id+'?tenant_id='+$progress.tenant_id)
 $names=@($taskDetail.work_orders|ForEach-Object {$_.contract.tool_policy.mcp_tools}|ForEach-Object {$_.tools}|Sort-Object -Unique)
 Check-Preparation ($names.Count -eq 1 -and $names[0] -eq 'echo') 'Agent received MCP tools beyond the explicit echo grant'
 $prepareEvent=@($effects|Where-Object {$_.role -eq 'handler' -and $_.details.tool -eq 'mcp.prepare'}|Select-Object -First 1)[0]
 $agentEvent=@($effects|Where-Object {$_.role -eq 'agent' -and $_.details.tool -eq 'mcp.call'}|Select-Object -First 1)[0]
 Check-Preparation ($prepareEvent.seq -lt $agentEvent.seq) 'Agent execution did not follow Handler preparation'
 $operatorInstalls=docker compose exec -T postgres psql -X -qAt -U echelon -d echelon -c "SELECT count(*) FROM audit_records WHERE tenant_id='$($progress.tenant_id)' AND actor_role='operator' AND action IN ('mcp.configured','mcp.installed');"
 if($LASTEXITCODE -ne 0){throw 'Owned preparation audit query failed'}
 Check-Preparation ([int]("$operatorInstalls".Trim()) -eq 0) 'An operator manually installed the OAuth scenario MCP'
 $files=@($taskDetail.artifacts|Where-Object {$_.work_order_id -and $_.name -eq 'mcp-prepared.txt'})
 Check-Preparation ($files.Count -eq 1) 'Automatic preparation lost or duplicated the recovered file'
 $file=Invoke-Echelon ('/v1/operator/artifacts/'+$files[0].id+'?tenant_id='+$progress.tenant_id)
 Check-Preparation ($file.content -ceq 'MCP_PREPARED_73') 'Recovered MCP echo output differs from the requested bytes'
 $oauth=@{passed=$true;assertions=$script:PreparationChecks;run_id=$progress.run_id;task_id=$progress.task_id;calls=$run.calls;handler_registry_and_preparation=$true;agent_mcp_effect=$true;technical_user_input_requests=0;actual_file_verified=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
 Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/mcp-preparation-oauth.json') ($oauth|ConvertTo-Json)
 $oauth|ConvertTo-Json -Compress
}
} finally {Pop-Location}
