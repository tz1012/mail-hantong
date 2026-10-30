#requires -Version 5.1
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-SafePath([string] $Path) {
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Linked paths are not supported: $cursor" }
        }
        $parent = [IO.Path]::GetDirectoryName($cursor)
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}
function Assert-Version([string] $Version) {
    if ($Version.Length -gt 64 -or $Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$') { throw 'Invalid package version' }
}
function Get-ChildPath([string] $Base, [string] $Relative) {
    $parts = $Relative.Replace('/', '\').Split('\')
    if ([string]::IsNullOrWhiteSpace($Relative)) { throw 'Empty relative path' }
    foreach ($part in $parts) {
        if ($part -in @('', '.', '..') -or $part -match '[<>:"|?*\x00-\x1f]' -or
            $part.EndsWith('.') -or $part.EndsWith(' ') -or
            $part -match '^(?i:con|prn|aux|nul|clock\$|com[1-9]|lpt[1-9])(\..*)?$') { throw 'Unsafe relative path' }
    }
    $baseFull = [IO.Path]::GetFullPath($Base).TrimEnd('\') + '\'
    $full = [IO.Path]::GetFullPath((Join-Path $Base ($parts -join '\')))
    if (-not $full.StartsWith($baseFull, [StringComparison]::OrdinalIgnoreCase)) { throw 'Path escapes its directory' }
    Assert-SafePath $full
    return $full
}
function Read-JsonFile([string] $Path) {
    Assert-SafePath $Path
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw 'Required manifest is missing' }
    if ((Get-Item -LiteralPath $Path).Length -gt 4194304) { throw 'Manifest is too large' }
    return (Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json)
}
function Assert-FileRecords($Files) {
    if ($Files -isnot [Array] -or $Files.Count -eq 0) { throw 'Files must be a nonempty array' }
    $seen = @{}
    foreach ($entry in $Files) {
        $relative = [string]$entry.path
        $null = Get-ChildPath $script:InstallBase $relative
        $key = $relative.Replace('/', '\').ToLowerInvariant()
        if ($seen.ContainsKey($key) -or [string]$entry.sha256 -notmatch '^[0-9a-fA-F]{64}$') { throw 'Duplicate file or invalid hash' }
        $seen[$key] = $true
    }
    foreach ($name in @('bin\hantong.exe', 'bin\mail-hantong-core.exe', 'license')) {
        if (-not $seen.ContainsKey($name)) { throw 'Required package file is missing' }
    }
}
function Assert-FileHash([string] $Path, [string] $Hash) {
    Assert-SafePath $Path
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf) -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $Hash) { throw "Owned file is missing or changed: $Path" }
}
function Get-Shim([string] $Binary) {
    return (@('@echo off', 'setlocal', 'set "HT_VERSION="',
        'if not exist "%~dp0..\current.txt" exit /b 1',
        'set /p HT_VERSION=<"%~dp0..\current.txt"', 'if not defined HT_VERSION exit /b 1',
        ('"%~dp0..\versions\%HT_VERSION%\bin\' + $Binary + '.exe" %*'),
        'exit /b %errorlevel%', '') -join "`r`n")
}
function Assert-Shims([string] $Root) {
    foreach ($binary in @('hantong', 'mail-hantong-core')) {
        $path = Get-ChildPath $Root "bin/$binary.cmd"
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            [IO.File]::ReadAllText($path) -cne (Get-Shim $binary)) { throw 'Installed launcher was changed' }
    }
}
function Read-InstallState([string] $Root) {
    $marker = Get-ChildPath $Root '.mail-hantong-owner'
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf) -or
        [IO.File]::ReadAllText($marker) -cne 'mail-hantong-cli-v1') { throw 'Installation ownership is missing' }
    $state = Read-JsonFile (Get-ChildPath $Root 'install-manifest.json')
    if ($state.schema_version -ne 1 -or $state.versions -isnot [Array] -or $state.versions.Count -eq 0) { throw 'Invalid installation state' }
    $seen = @{}
    foreach ($version in $state.versions) {
        Assert-Version ([string]$version.version)
        if ($seen.ContainsKey([string]$version.version)) { throw 'Duplicate installed version' }
        $seen[[string]$version.version] = $true
        Assert-FileRecords $version.files
        $directory = Get-ChildPath $Root "versions/$($version.version)"
        foreach ($file in $version.files) { Assert-FileHash (Get-ChildPath $directory $file.path) $file.sha256 }
    }
    Assert-Shims $Root
    $current = Get-ChildPath $Root 'current.txt'
    if (Test-Path -LiteralPath $current -PathType Leaf) {
        $active = [IO.File]::ReadAllText($current)
        Assert-Version $active
        if (-not $seen.ContainsKey($active)) { throw 'Active version is not owned' }
    }
    return $state
}
function Write-AtomicText([string] $Path, [string] $Text) {
    Assert-SafePath $Path
    $temporary = "$Path.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.File]::WriteAllText($temporary, $Text, (New-Object Text.UTF8Encoding($false)))
        Assert-SafePath $Path
        if (Test-Path -LiteralPath $Path) { [IO.File]::Replace($temporary, $Path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $Path) }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Assert-SafePath $temporary; Remove-Item -LiteralPath $temporary -Force }
    }
}
function Remove-OwnedFiles([string] $Directory, $Files) {
    $paths = @($Files | ForEach-Object {
        $path = Get-ChildPath $Directory $_.path
        Assert-FileHash $path $_.sha256
        $path
    })
    $directories = @{}
    foreach ($path in $paths) {
        Assert-SafePath $path
        Remove-Item -LiteralPath $path -Force
        $parent = [IO.Path]::GetDirectoryName($path)
        while ($parent.Length -ge $Directory.Length) {
            $directories[$parent] = $true
            if ($parent -eq $Directory) { break }
            $parent = [IO.Path]::GetDirectoryName($parent)
        }
    }
    foreach ($path in ($directories.Keys | Sort-Object Length -Descending)) {
        Assert-SafePath $path
        if ((Test-Path -LiteralPath $path -PathType Container) -and @(Get-ChildItem -LiteralPath $path -Force).Count -eq 0) {
            Remove-Item -LiteralPath $path -Force
        }
    }
}
