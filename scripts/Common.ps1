Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
$script:ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$script:RuntimeRoot = Join-Path $script:ProjectRoot '.runtime'
$script:ApiBase = 'http://127.0.0.1:18431'

function Initialize-PrivateDirectory {
    param([string]$Path)
    $absolute = [IO.Path]::GetFullPath($Path)
    $expected = [IO.Path]::GetFullPath((Join-Path $script:ProjectRoot '.runtime'))
    if ($absolute -ne $expected -and -not $absolute.StartsWith($expected + [IO.Path]::DirectorySeparatorChar)) {
        throw 'Private directory is outside the project runtime directory.'
    }
    [IO.Directory]::CreateDirectory($absolute) | Out-Null
    $acl = Get-Acl -LiteralPath $absolute
    $acl.SetAccessRuleProtection($true, $false)
    $userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    foreach ($sid in @($userSid, [Security.Principal.SecurityIdentifier]::new('S-1-5-18'))) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.SetAccessRule($rule)
    }
    Set-Acl -LiteralPath $absolute -AclObject $acl
}
function Write-PrivateText {
    param([string]$Path, [string]$Value)
    [IO.File]::WriteAllText($Path, $Value, [Text.UTF8Encoding]::new($false))
}
function New-RuntimeSecret {
    $bytes = [byte[]]::new(48)
    [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    [Convert]::ToBase64String($bytes)
}
function Protect-RuntimeBytes {
    param([byte[]]$Bytes, [string]$Destination)
    $sealed = [Security.Cryptography.ProtectedData]::Protect(
        $Bytes, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
    [IO.File]::WriteAllBytes($Destination, $sealed)
}
function Unprotect-RuntimeBytes {
    param([string]$Path)
    [Security.Cryptography.ProtectedData]::Unprotect(
        [IO.File]::ReadAllBytes($Path), $null,
        [Security.Cryptography.DataProtectionScope]::CurrentUser)
}
function Get-OperatorToken {
    [Text.Encoding]::UTF8.GetString((Unprotect-RuntimeBytes (Join-Path $script:RuntimeRoot 'operator.dpapi')))
}
function Invoke-Echelon {
    param([string]$Path, [string]$Method='GET', $Body, [string]$Token)
    if (-not $Token) { $Token = Get-OperatorToken }
    $requestOptions = @{
        Uri = $script:ApiBase + $Path
        Method = $Method
        Headers = @{ Authorization = 'Bearer ' + $Token }
        TimeoutSec = 150
    }
    if ($null -ne $Body) {
        $requestOptions.ContentType = 'application/json; charset=utf-8'
        $requestOptions.Body = [Text.Encoding]::UTF8.GetBytes(($Body | ConvertTo-Json -Depth 16 -Compress))
    }
    Invoke-RestMethod @requestOptions
}
function Invoke-Compose {
    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Arguments)
    Push-Location $script:ProjectRoot
    try {
        & docker compose @Arguments
        if ($LASTEXITCODE -ne 0) { throw 'An Echelon Docker Compose command failed.' }
    } finally { Pop-Location }
}
