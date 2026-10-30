#requires -Version 5.1
[CmdletBinding()]
param(
    [string] $Source,
    [string] $InstallRoot = (Join-Path $env:LOCALAPPDATA 'Programs\mail-hantong-cli'),
    [switch] $AddToPath,
    [string] $PackageRoot
)
# Windows PowerShell 5.1 -File does not initialize PSScriptRoot while binding
# default parameter expressions. Resolve the omitted source in the script body.
if (-not $PSBoundParameters.ContainsKey('PackageRoot')) { $PackageRoot = $PSScriptRoot }
if (-not $PSBoundParameters.ContainsKey('Source')) { $Source = $PackageRoot }
# The bootstrap invokes these checksum-verified scripts in memory. This also
# works under the normal Restricted script-file policy without changing it.
. ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $PackageRoot 'package-common.ps1'))))
$script:InstallBase = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$root = $script:InstallBase
$sourceFull = [IO.Path]::GetFullPath($Source)
Assert-SafePath $root
Assert-SafePath $sourceFull
$release = Read-JsonFile (Join-Path $sourceFull 'release-manifest.json')
if ($release.format_version -ne 1 -or $release.target -cne 'windows-x86_64') { throw 'Unsupported release manifest' }
Assert-Version ([string]$release.version)
Assert-FileRecords $release.files
foreach ($file in $release.files) { Assert-FileHash (Get-ChildPath $sourceFull $file.path) $file.sha256 }
$marker = Get-ChildPath $root '.mail-hantong-owner'
$statePath = Get-ChildPath $root 'install-manifest.json'
if ((Test-Path -LiteralPath $root) -and -not (Test-Path -LiteralPath $marker) -and
    @(Get-ChildItem -LiteralPath $root -Force | Where-Object Name -ne '.install.lock').Count -gt 0) {
    throw 'Refusing to claim a nonempty unowned directory'
}
$null = New-Item -ItemType Directory -Force -Path $root
$lockPath = Get-ChildPath $root '.install.lock'
$lock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
$stage = $null
$createdFiles = @()
$createdShims = @()
$promoted = $false
$recorded = $false
$firstInstall = $false
try {
    $versions = @()
    if (Test-Path -LiteralPath $marker) {
        $state = Read-InstallState $root
        $versions = @($state.versions)
    } else {
        if (@(Get-ChildItem -LiteralPath $root -Force | Where-Object Name -ne '.install.lock').Count -gt 0) { throw 'Unowned installation content' }
        $firstInstall = $true
    }
    $versionDir = Get-ChildPath $root "versions/$($release.version)"
    $existing = @($versions | Where-Object version -CEQ $release.version)
    if (Test-Path -LiteralPath $versionDir) {
        if ($existing.Count -ne 1 -or $existing[0].files.Count -ne $release.files.Count) { throw 'Existing version is unmanaged or differs' }
        $old = @{}
        foreach ($file in $existing[0].files) { $old[$file.path.Replace('/', '\')] = $file.sha256 }
        foreach ($file in $release.files) {
            if ($old[$file.path.Replace('/', '\')] -ine $file.sha256) { throw 'Existing version differs' }
            Assert-FileHash (Get-ChildPath $versionDir $file.path) $file.sha256
        }
    } else {
        $stage = Get-ChildPath $root "staging/$([guid]::NewGuid().ToString('N'))"
        $null = New-Item -ItemType Directory -Force -Path $stage
        foreach ($file in $release.files) {
            $destination = Get-ChildPath $stage $file.path
            $null = New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($destination))
            [IO.File]::Copy((Get-ChildPath $sourceFull $file.path), $destination, $false)
            $createdFiles += $file
            Assert-FileHash $destination $file.sha256
        }
        $null = New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($versionDir))
        Assert-SafePath $versionDir
        [IO.Directory]::Move($stage, $versionDir)
        $stage = $null
        $promoted = $true
        $versions += [pscustomobject]@{ version = $release.version; files = @($release.files) }
    }
    $bin = Get-ChildPath $root 'bin'
    $null = New-Item -ItemType Directory -Force -Path $bin
    if ($firstInstall) {
        Write-AtomicText $marker 'mail-hantong-cli-v1'
    }
    foreach ($binary in (Get-OwnedBinaries $versions)) {
        $shim = Get-ChildPath $bin "$binary.cmd"
        if (Test-Path -LiteralPath $shim) {
            if ([IO.File]::ReadAllText($shim) -cne (Get-Shim $binary)) { throw 'Refusing to replace an unmanaged launcher' }
        } else {
            Write-AtomicText $shim (Get-Shim $binary)
            $createdShims += $shim
        }
    }
    $stateText = [ordered]@{ schema_version = 1; versions = @($versions) } | ConvertTo-Json -Depth 12
    # An interrupted update can leave an owned inactive version. Only this
    # final pointer replacement activates it; the previous version stays usable.
    Write-AtomicText $statePath $stateText
    $recorded = $true
    if ($AddToPath) { Add-OwnedUserPath $root }
    Write-AtomicText (Get-ChildPath $root 'current.txt') ([string]$release.version)
} catch {
    if (-not $recorded) {
        foreach ($shim in $createdShims) { Assert-SafePath $shim; Remove-Item -LiteralPath $shim -Force }
    }
    if ($stage -and (Test-Path -LiteralPath $stage)) { Remove-OwnedFiles $stage $createdFiles }
    if ($promoted -and -not $recorded) { Remove-OwnedFiles $versionDir $release.files }
    if ($firstInstall -and -not $recorded) {
        foreach ($relative in @('bin/hantong.cmd', 'bin/mail-hantong-core.cmd', '.mail-hantong-owner')) {
            $path = Get-ChildPath $root $relative
            if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force }
        }
        foreach ($relative in @('bin', 'versions', 'staging')) {
            $path = Get-ChildPath $root $relative
            if ((Test-Path -LiteralPath $path -PathType Container) -and @(Get-ChildItem -LiteralPath $path -Force).Count -eq 0) {
                Remove-Item -LiteralPath $path -Force
            }
        }
    }
    throw
} finally {
    $lock.Dispose()
}
$escapedBin = $bin.Replace("'", "''")
Write-Output "Installed mail-hantong CLI $($release.version)."
Write-Output "For this PowerShell session: `$env:Path = '$escapedBin;' + `$env:Path"
Write-Output "Uninstall with: & '$($versionDir.Replace("'", "''"))\uninstall.ps1' -InstallRoot '$($root.Replace("'", "''"))'"
if ($AddToPath) { Write-Output 'User PATH is configured. Restart existing terminals to reload their environment.' }
else { Write-Output 'No persistent PATH settings were changed.' }
Write-Output 'No startup, account or mail-data settings were changed.'
