. (Join-Path $PSScriptRoot 'Common.ps1')
Push-Location $script:ProjectRoot
try {
    $paths = @(git ls-files --cached --others --exclude-standard)
    $patterns = @(
        '-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',
        'gh[pousr]_[A-Za-z0-9]{30,}',
        'github_pat_[A-Za-z0-9_]{30,}',
        'sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{35,}',
        'eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}'
    )
    $hits = @()
    foreach ($path in $paths) {
        if ($path -match '(^|/)\.runtime/|(^|/)\.build/|(^|/)\.env($|\.)') {
            $hits += @{path=$path;reason='private_path'}
            continue
        }
        $content = [IO.File]::ReadAllText((Join-Path $script:ProjectRoot $path))
        foreach ($pattern in $patterns) {
            if ($content -match $pattern) { $hits += @{path=$path;reason='credential_pattern'}; break }
        }
        if ($content -match 'chatgpt\.com/space/page_|codex:/{2}threads/|C:\\Users\\Administrator') {
            $hits += @{path=$path;reason='private_reference'}
        }
    }
    $privateIgnored = @()
    foreach ($path in @('.runtime/bootstrap/postgres-password','.runtime/bootstrap/vault-core-token','.runtime/operator.dpapi','.build/build.log')) {
        git check-ignore --quiet $path
        $privateIgnored += ($LASTEXITCODE -eq 0)
    }
    if ($hits.Count -gt 0 -or $privateIgnored -contains $false) {
        @{passed=$false;findings=$hits;private_paths_ignored=($privateIgnored -notcontains $false)} | ConvertTo-Json -Depth 4
        throw 'Public content verification failed.'
    }
    @{passed=$true;files=$paths.Count;credential_pattern_findings=0;private_paths_ignored=$true} | ConvertTo-Json -Compress
} finally { Pop-Location }
