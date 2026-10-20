. (Join-Path $PSScriptRoot 'Common.ps1')
Initialize-PrivateDirectory (Join-Path $script:RuntimeRoot 'evidence')
$checks=0
function Check-File([bool]$Value,[string]$Message){if(-not $Value){throw $Message};$script:checks++}
function Reject-File([string]$Path,$Body,[string]$Code){
 $rejected=$false
 try{$null=Invoke-Echelon $Path 'POST' $Body}catch{$rejected=$_.ErrorDetails.Message -match [regex]::Escape($Code)}
 Check-File $rejected "Expected attachment rejection: $Code"
}
$tenant=(Invoke-Echelon '/v1/operator/tenants' 'POST' @{name='Owned Task attachment boundaries'}).id
$bytes=[byte[]]::new(1048576);[Array]::Fill[byte]($bytes,[byte]255)
$encoded=[Convert]::ToBase64String($bytes)
$request=@{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal='Owned attachment boundary test. These binary fixtures are test inputs; no business action is required.';attachments=@(
 @{request_id=[guid]::NewGuid().ToString();name='large-a.bin';media_type='application/octet-stream';data_base64=$encoded},
 @{request_id=[guid]::NewGuid().ToString();name='large-b.bin';media_type='application/octet-stream';data_base64=$encoded}
)}
# The valid base64 request exceeds the ordinary 2 MiB JSON limit.
$launch=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' $request
$null=Invoke-Echelon "/v1/operator/runs/$($launch.run_id)/control" 'POST' @{action='cancel'}
$again=Invoke-Echelon '/v1/operator/tasks/launch' 'POST' $request
Check-File ($again.replayed -and $again.task_id -eq $launch.task_id -and $again.run_id -eq $launch.run_id) 'Large attachment retry duplicated a Task or run.'
$task=Invoke-Echelon "/v1/operator/tasks/$($launch.task_id)?tenant_id=$tenant"
Check-File ($task.artifacts.Count -eq 2) 'Large file upload was incomplete or duplicated.'
foreach($f in $task.artifacts){
 $stored=Invoke-Echelon "/v1/operator/artifacts/$($f.id)?tenant_id=$tenant"
 Check-File ($f.bytes -eq 1048576 -and $stored.data_base64 -eq $encoded) 'Stored binary bytes differ.'
}
$download=Join-Path $script:RuntimeRoot 'evidence/attachment-download.bin'
$null=Invoke-WebRequest -Uri ($script:ApiBase+"/v1/operator/artifacts/$($task.artifacts[0].id)/download?tenant_id=$tenant") -Headers @{Authorization='Bearer '+(Get-OperatorToken)} -OutFile $download
Check-File ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -eq $task.artifacts[0].sha256) 'Downloaded bytes differ from the immutable artifact hash.'
$invalid=@{tenant_id=$tenant;request_id=[guid]::NewGuid().ToString();goal='Owned invalid attachment fixture';attachments=@(@{request_id=[guid]::NewGuid().ToString();name='../escape.txt';content='invalid'})}
Reject-File '/v1/operator/tasks/launch' $invalid 'invalid_workspace_input'
$invalid.attachments=@(@{request_id=[guid]::NewGuid().ToString();name='large.txt';content=('x'*1048577)})
Reject-File '/v1/operator/tasks/launch' $invalid 'artifact_budget_exceeded'
$invalid.attachments=@(@{request_id=[guid]::NewGuid().ToString();name='broken.bin';data_base64='not valid base64!'})
Reject-File '/v1/operator/tasks/launch' $invalid 'invalid_workspace_input'
$invalid.attachments=@(0..16|ForEach-Object{@{request_id=[guid]::NewGuid().ToString();name="input-$_.txt";content=''}})
Reject-File '/v1/operator/tasks/launch' $invalid 'attachment_budget_exceeded'
$invalid.attachments=@(0..4|ForEach-Object{@{request_id=[guid]::NewGuid().ToString();name="input-$_.txt";content=('x'*1048576)}})
Reject-File '/v1/operator/tasks/launch' $invalid 'attachment_budget_exceeded'
$invalid.attachments=@(@{request_id=[guid]::NewGuid().ToString();name='same.txt';content='a'},@{request_id=[guid]::NewGuid().ToString();name='same.txt';content='b'})
Reject-File '/v1/operator/tasks/launch' $invalid 'attachment_duplicate'
$list=Invoke-Echelon "/v1/operator/tasks?tenant_id=$tenant"
Check-File ($list.tasks.Count -eq 1) 'An invalid attachment left a partially created Task.'
$input=@{request_id=[guid]::NewGuid().ToString();name='첨부 자료.txt';content='가😀나다'}
$uploaded=Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$launch.task_id;artifact=$input}
$replay=Invoke-Echelon '/v1/operator/artifacts' 'POST' @{tenant_id=$tenant;task_id=$launch.task_id;artifact=$input}
Check-File ($replay.replayed -and $replay.artifact.id -eq $uploaded.artifact.id) 'Single-file retry duplicated the attachment.'
$handler=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='handler';tenant_id=$tenant;task_id=$launch.task_id}).token
$preview=Invoke-Echelon '/v1/tools/invoke' 'POST' @{tool='artifact.read';tenant_id=$tenant;task_id=$launch.task_id;input=@{artifact_id=$uploaded.artifact.id;offset=1;limit=2}} $handler
Check-File ($preview.content -eq '😀나' -and $preview.truncated -and $preview.next_offset -eq 3) 'Unicode attachment preview split a character or lost its cursor.'
Check-File (-not ($preview.PSObject.Properties.Name -contains 'data_base64')) 'Preview exposed the full encoded file.'
$binary=Invoke-Echelon '/v1/tools/invoke' 'POST' @{tool='artifact.read';tenant_id=$tenant;task_id=$launch.task_id;input=@{artifact_id=$task.artifacts[0].id;limit=4000}} $handler
Check-File (-not $binary.text_readable -and $null -eq $binary.content) 'Binary input was presented as extracted text.'
$other=(Invoke-Echelon '/v1/operator/tasks' 'POST' @{tenant_id=$tenant;tenant_name='Owned attachment fixture';goal='Foreign Task file boundary'}).task_id
$otherToken=(Invoke-Echelon '/v1/operator/role-tokens' 'POST' @{role='handler';tenant_id=$tenant;task_id=$other}).token
$denied=$false
try{$null=Invoke-Echelon '/v1/tools/invoke' 'POST' @{tool='artifact.read';tenant_id=$tenant;task_id=$other;input=@{artifact_id=$uploaded.artifact.id;limit=4000}} $otherToken}catch{$denied=[int]$_.Exception.Response.StatusCode -in @(403,404)}
Check-File $denied 'Another Task read a private attachment.'
$result=@{passed=$true;assertions=$checks;tenant_id=$tenant;task_id=$launch.task_id;large_request_over_2mib=$true;binary_download_hash_verified=$true;invalid_inputs_atomic=$true;scoped_previews=$true;verified_at=[DateTimeOffset]::UtcNow.ToString('o')}
Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/attachments.json') ($result|ConvertTo-Json -Depth 8)
$result|ConvertTo-Json -Compress
