param([switch]$VerifyRestore)
. (Join-Path $PSScriptRoot 'Common.ps1')
Push-Location $script:ProjectRoot
try {
$active=& docker compose exec -T postgres psql -U echelon -d echelon -Atc "SELECT count(*) FROM execution_runs WHERE status IN ('queued','running')"
if($LASTEXITCODE -ne 0 -or [long]("$active".Trim()) -ne 0){throw 'Pause and reconcile active Echelon work before taking a consistent backup.'}
if($VerifyRestore){
 $verificationRunning=@(& docker compose -f compose.verify.yaml ps --status running -q)
 if($LASTEXITCODE -ne 0 -or $verificationRunning.Count){throw 'The isolated verification project must be stopped before restore verification.'}
}
$volumeOwner=& docker volume inspect echelon-nano_vault-data --format '{{index .Labels "com.docker.compose.project"}}'
if($LASTEXITCODE -ne 0 -or "$volumeOwner".Trim() -ne 'echelon-nano'){throw 'The Vault volume does not belong to this project.'}
$stamp=(Get-Date -Format 'yyyyMMdd-HHmmss')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
$databaseDirectory=Join-Path $script:RuntimeRoot ('backups/'+$stamp)
$vaultDirectory=Join-Path $script:RuntimeRoot ('vault-backups/'+$stamp)
Initialize-PrivateDirectory $databaseDirectory
Initialize-PrivateDirectory $vaultDirectory
$dumpPath='/tmp/echelon-backup-'+$stamp+'.dump'
$stopped=$false
try{
 Invoke-Compose @('stop','core');$stopped=$true
 & docker compose exec -T postgres pg_dump -U echelon -d echelon -Fc -f $dumpPath
 if($LASTEXITCODE -ne 0){throw 'Database snapshot failed.'}
 & docker compose cp ("postgres:"+$dumpPath) (Join-Path $databaseDirectory 'knowledge-and-artifacts.dump')
 if($LASTEXITCODE -ne 0){throw 'Database snapshot export failed.'}
 & docker compose exec -T postgres rm $dumpPath
 if($LASTEXITCODE -ne 0){throw 'Temporary database snapshot cleanup failed.'}
 Invoke-Compose @('stop','vault')
 & docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges:true --user vault --mount 'type=volume,source=echelon-nano_vault-data,target=/vault/data,readonly' --mount ("type=bind,source="+$vaultDirectory+",target=/backup") --entrypoint /bin/sh hashicorp/vault:1.20.4 -c 'tar -cf /backup/vault-storage.tar -C /vault/data .'
 if($LASTEXITCODE -ne 0){throw 'Encrypted Vault storage snapshot failed.'}
 $dump=Join-Path $databaseDirectory 'knowledge-and-artifacts.dump'
 $storage=Join-Path $vaultDirectory 'vault-storage.tar'
 # Validate the exported custom-format archive without restoring or modifying data.
 $validationPath='/tmp/echelon-validate-'+$stamp+'.dump'
 & docker compose cp $dump ("postgres:"+$validationPath)
 if($LASTEXITCODE -ne 0){throw 'Backup validation copy failed.'}
 try{$archiveEntries=@(& docker compose exec -T postgres pg_restore -l $validationPath);if($LASTEXITCODE -ne 0 -or -not @($archiveEntries|Where-Object {$_ -match 'TABLE DATA public knowledge_revisions'}).Count){throw 'Database archive inventory validation failed.'}}finally{& docker compose exec -T postgres rm $validationPath |Out-Null}
 $restoreVerified=$false
 if($VerifyRestore){
  try{
   & docker compose -f compose.verify.yaml up -d database
   if($LASTEXITCODE -ne 0){throw 'Isolated restore database startup failed.'}
   for($attempt=0;$attempt -lt 30;$attempt++){& docker compose -f compose.verify.yaml exec -T database pg_isready -U verifier -d echelon_verification *> $null;if($LASTEXITCODE -eq 0){break};Start-Sleep -Milliseconds 500}
   $restoreIdentity=& docker compose -f compose.verify.yaml exec -T database psql -U verifier -d echelon_verification -Atc 'SELECT current_database()'
   $tableCount=& docker compose -f compose.verify.yaml exec -T database psql -U verifier -d echelon_verification -Atc "SELECT count(*) FROM pg_tables WHERE schemaname='public'"
   if("$restoreIdentity".Trim() -ne 'echelon_verification' -or [long]("$tableCount".Trim()) -ne 0){throw 'Restore target must be the empty isolated verification database.'}
   & docker compose -f compose.verify.yaml cp $dump database:/tmp/owned-restore.dump
   if($LASTEXITCODE -ne 0){throw 'Isolated restore archive transfer failed.'}
   & docker compose -f compose.verify.yaml exec -T database pg_restore --no-owner --no-acl --exit-on-error -U verifier -d echelon_verification /tmp/owned-restore.dump
   if($LASTEXITCODE -ne 0){throw 'Isolated restore failed.'}
   $countSql="SELECT (SELECT count(*) FROM knowledge_revisions)::text||':'||(SELECT count(*) FROM artifacts)::text||':'||(SELECT count(*) FROM execution_runs)::text"
   $sourceCounts=& docker compose exec -T postgres psql -U echelon -d echelon -Atc $countSql
   $restoredCounts=& docker compose -f compose.verify.yaml exec -T database psql -U verifier -d echelon_verification -Atc $countSql
   if($LASTEXITCODE -ne 0 -or "$sourceCounts".Trim() -ne "$restoredCounts".Trim()){throw 'Restored knowledge/artifact/run counts differ.'}
   $restoreVerified=$true
  }finally{& docker compose -f compose.verify.yaml down --remove-orphans |Out-Null}
 }
 $result=@{passed=$true;database_sha256=(Get-FileHash -LiteralPath $dump -Algorithm SHA256).Hash;vault_storage_sha256=(Get-FileHash -LiteralPath $storage -Algorithm SHA256).Hash;database_bytes=(Get-Item -LiteralPath $dump).Length;vault_storage_bytes=(Get-Item -LiteralPath $storage).Length;separate_storage_archives=$true;recovery_material_included=$false;isolated_database_restore_verified=$restoreVerified;vault_restore_performed=$false;created_at=[DateTimeOffset]::UtcNow.ToString('o')}
 Write-PrivateText (Join-Path $databaseDirectory 'manifest.json') ($result|ConvertTo-Json)
 Write-PrivateText (Join-Path $script:RuntimeRoot 'evidence/backup.json') ($result|ConvertTo-Json)
}finally{if($stopped){& (Join-Path $PSScriptRoot 'Initialize-Local.ps1') -SkipBuild}}
$result|ConvertTo-Json -Compress
}finally{Pop-Location}
