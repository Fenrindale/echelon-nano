param([switch]$WithOAuth,[switch]$SkipStdio)
. (Join-Path $PSScriptRoot 'Common.ps1')
if($script:ApiBase -ne 'http://127.0.0.1:18432'){throw 'Run MCP synthetic verification only in the isolated 18432 copy.'}
$script:checks=0
function Assert-Mcp($condition,[string]$message){if(-not $condition){throw $message};$script:checks++}
function Domain-Mcp($task,$tool,$inputData){Invoke-Echelon '/v1/operator/domain' 'POST' @{tenant_id=$tenant;task_id=$task;tool=$tool;input=$inputData}}
function Agent-Mcp($tool,$inputData){Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$task.task_id;target='owned:mcp';tool=$tool;input=$inputData} $agent}
function Deny-Mcp($roleToken,$tool,$inputData,$expected=403){
 $response=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/tools/invoke') -Method POST -Headers @{Authorization='Bearer '+$roleToken} -ContentType 'application/json' -Body (@{tenant_id=$tenant;task_id=$task.task_id;target='owned:mcp';tool=$tool;input=$inputData}|ConvertTo-Json -Depth 30) -SkipHttpErrorCheck
 Assert-Mcp ([int]$response.StatusCode -eq $expected) ('Unexpected denial status for '+$tool+': '+$response.StatusCode)
 return ($response.Content|ConvertFrom-Json)
}
$tenant=[guid]::NewGuid().ToString()
$source=Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned MCP verification';goal='Import owned synthetic MCP credentials';contract=@{kind='work';allowed_tools=@('credential.import')}}
$inputFile=Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$source.task_id;artifact=@{request_id=[guid]::NewGuid().ToString();name='owned.env';content="MCP_TOKEN=owned-mcp-verification-value`nESET_REGION=owned-synthetic-region"}}
$null=Invoke-Echelon ('/v1/operator/tasks/'+$source.task_id) 'PATCH' @{tenant_id=$tenant;contract=@{kind='work';allowed_tools=@('credential.import');input_artifacts=@($inputFile.artifact.id)};reason='Owned import grant'}
$importOrder=Domain-Mcp $source.task_id 'agent.delegate' @{target='owned:import';objective='Import synthetic env';expected_output='Storage receipt';request_id=[guid]::NewGuid().ToString()}
$importAgent=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$source.task_id;work_order_id=$importOrder.work_order_id;targets=@('owned:import')}).token
$bundle=(Invoke-Echelon '/v1/tools/invoke' 'POST' @{tenant_id=$tenant;task_id=$source.task_id;target='owned:import';tool='credential.import';input=@{artifact_id=$inputFile.artifact.id;request_id=[guid]::NewGuid().ToString()}} $importAgent).bundle.id
$binding=@(@{name='MCP_AUTH_TOKEN';bundle_id=$bundle;variable='MCP_TOKEN'})
function New-McpServer([string]$name,$spec){
 $saved=Invoke-Echelon '/v1/operator/mcp' 'POST' @{tenant_id=$tenant;name=$name;enabled=$true;spec=$spec}
 $ready=Invoke-Echelon ('/v1/operator/mcp/'+$saved.id+'/install') 'POST' @{tenant_id=$tenant;expected_version=$saved.version}
 Assert-Mcp ($ready.status -eq 'ready' -and @($ready.tools).Count -gt 0) 'MCP did not initialize and discover actual tools.'
 return @{id=$saved.id;version=$saved.version;tools=$ready.tools;spec=$spec;name=$name}
}
$remote=New-McpServer 'Owned work items' @{transport='http';url='http://fixture:8081/mcp';allow_private=$true;allow_http=$true;environment=$binding}
$lost=New-McpServer 'Owned lost reply' @{transport='http';url='http://fixture:8081/mcp-lost';allow_private=$true;allow_http=$true;environment=@()}
$grants=@(@{server_id=$remote.id;version=$remote.version;tools=@('list_items','resolve_item')},@{server_id=$lost.id;version=$lost.version;tools=@('resolve_item')})
$stdio=$null
$pythonServer=$null
if(-not $SkipStdio){
 $stdio=New-McpServer 'Owned stdio example' @{transport='npm';package='@modelcontextprotocol/server-everything';package_version='2.0.0';executable='mcp-server-everything';args=@();environment=@()}
 Assert-Mcp (@($stdio.tools|Where-Object name -eq 'echo').Count -eq 1) 'Official npm example echo tool missing.'
 $grants+=@{server_id=$stdio.id;version=$stdio.version;tools=@('echo')}
 $pythonServer=New-McpServer 'Owned Python time' @{transport='python';package='mcp-server-time';package_version='2026.10.19';executable='mcp_server_time';args=@();environment=@()}
 $grants+=@{server_id=$pythonServer.id;version=$pythonServer.version;tools=@('get_current_time')}
}
$contract=@{kind='work';allowed_tools=@('mcp.call','artifact.write');mcp_tools=$grants;credential_bundles=@($bundle);output_format='Owned verification report';max_work_orders=4}
$task=Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned MCP verification';goal='Resolve only owned synthetic MCP work items';contract=$contract}
$metadata=Domain-Mcp $task.task_id 'credential.inspect' @{bundle_id=$bundle}
Assert-Mcp ($metadata.manifest.values_withheld -and $metadata.configuration.ESET_REGION -eq 'owned-synthetic-region') 'Reusable cross-Task bundle metadata failed.'
Assert-Mcp (($metadata|ConvertTo-Json -Depth 20) -notmatch 'owned-mcp-verification-value') 'Credential metadata leaked a value.'
$order=Domain-Mcp $task.task_id 'agent.delegate' @{target='owned:mcp';objective='Use owned synthetic MCP';expected_output='Confirmed effect receipts';request_id=[guid]::NewGuid().ToString()}
$agent=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='agent';tenant_id=$tenant;task_id=$task.task_id;work_order_id=$order.work_order_id;targets=@('owned:mcp')}).token
$read=Agent-Mcp 'work_order.read' @{work_order_id=$order.work_order_id}
Assert-Mcp ($read.contract.mcp_schemas[0].tools[0].inputSchema.type -eq 'object') 'Frozen MCP input schemas missing.'
$key=[guid]::NewGuid().ToString()
$call=@{server_id=$remote.id;tool='list_items';arguments=@{};request_id=$key}
$first=Agent-Mcp 'mcp.call' $call
$again=Agent-Mcp 'mcp.call' $call
Assert-Mcp (($first|ConvertTo-Json -Depth 20) -eq ($again|ConvertTo-Json -Depth 20)) 'Completed MCP call was not reused exactly.'
Assert-Mcp (($first|ConvertTo-Json -Depth 20) -notmatch 'owned-mcp-verification-value') 'MCP output echoed a Vault secret.'
$resolved=Agent-Mcp 'mcp.call' @{server_id=$remote.id;tool='resolve_item';arguments=@{id='owned-item-A'};request_id=[guid]::NewGuid().ToString()}
Assert-Mcp ($resolved.result.content[0].text -match 'owned-item-A') 'Actual owned write did not return confirmation.'
if($stdio){$echo=Agent-Mcp 'mcp.call' @{server_id=$stdio.id;tool='echo';arguments=@{message='owned-stdio-proof'};request_id=[guid]::NewGuid().ToString()};Assert-Mcp ($echo.result.content[0].text -match 'owned-stdio-proof') 'Installed stdio execution failed.'}
if($pythonServer){$time=Agent-Mcp 'mcp.call' @{server_id=$pythonServer.id;tool='get_current_time';arguments=@{timezone='UTC'};request_id=[guid]::NewGuid().ToString()};Assert-Mcp ($time.result.content[0].text -match 'UTC') 'Installed Python stdio execution failed.'}
foreach($role in @('analyst','handler','master')){
 $token=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role=$role;tenant_id=$tenant;task_id=$task.task_id}).token
 $null=Deny-Mcp $token 'mcp.call' $call
 if($role -ne 'handler'){$null=Deny-Mcp $token 'mcp.inspect' @{server_id=$remote.id}}
}
$null=Deny-Mcp $agent 'mcp.call' @{server_id=$remote.id;tool='not_granted';arguments=@{};request_id=[guid]::NewGuid().ToString()}
$lostCall=@{server_id=$lost.id;tool='resolve_item';arguments=@{id='owned-unknown-outcome'};request_id=[guid]::NewGuid().ToString()}
$null=Deny-Mcp $agent 'mcp.call' $lostCall 400
$retry=Deny-Mcp $agent 'mcp.call' $lostCall 400
Assert-Mcp ($retry.error.code -eq 'mcp_outcome_unknown_do_not_retry') 'Unknown mutation was not protected from replay.'
$lostCall.request_id=[guid]::NewGuid().ToString()
$retry=Deny-Mcp $agent 'mcp.call' $lostCall 400
Assert-Mcp ($retry.error.code -eq 'mcp_outcome_unknown_do_not_retry') 'A new model call key replayed the same unknown mutation.'
$collected=Domain-Mcp $task.task_id 'work_order.read' @{work_order_id=$order.work_order_id}
Assert-Mcp (@($collected.tool_evidence.mcp_calls|Where-Object status -eq 'unknown').Count -eq 1) 'Unknown outcome receipt missing.'
$crossTenant=[guid]::NewGuid().ToString()
$response=Invoke-WebRequest -Uri ($script:ApiBase+'/v1/operator/mcp?tenant_id='+$crossTenant) -Headers @{Authorization='Bearer '+(Get-OperatorToken)}
Assert-Mcp ((($response.Content|ConvertFrom-Json).servers|Measure-Object).Count -eq 0) 'MCP inventory leaked across tenants.'
if($WithOAuth){
 $launched=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' @{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal='공통 MCP Owned work items에서 가상 업무 항목을 조회하고 owned-item-B를 처리한 다음 실제 MCP 영수증에 근거한 한국어 Markdown 결과 파일을 만들어 주세요. 이 서버의 모든 항목은 소유한 합성 검증 자료입니다. 설치된 공통 도구와 기존 Vault 설정을 직접 사용하고 추가 사용자 입력 없이 완료하세요. 다른 MCP 서버는 사용하지 마세요.'}
 Write-Host ('OAuth MCP run '+$launched.run_id)
 $deadline=(Get-Date).AddMinutes(10);$previous=''
 do {Start-Sleep -Seconds 4;$run=Invoke-Echelon ('/v1/operator/runs/'+$launched.run_id);$state=$run.status+':'+$run.phase+':'+$run.calls;if($state-ne$previous){Write-Host $state;$previous=$state}} while($run.status -in @('queued','running') -and (Get-Date)-lt$deadline)
 Assert-Mcp ($run.status -eq 'completed') ('OAuth MCP run did not complete: '+$run.status+' '+$run.error_code)
 Assert-Mcp ($run.result.outcome -eq 'completed') 'Original synthetic goal was not completed.'
}
$evidence=@{checks=$script:checks;tenant=$tenant;task=$task.task_id;stdio_verified=(-not $SkipStdio);oauth_verified=[bool]$WithOAuth;at=(Get-Date).ToUniversalTime().ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'mcp-verification.json') ($evidence|ConvertTo-Json)
$evidence|ConvertTo-Json
