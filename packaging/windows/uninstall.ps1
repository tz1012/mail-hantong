#requires -Version 5.1
[CmdletBinding()]
param([string] $InstallRoot = (Join-Path $env:LOCALAPPDATA 'Programs\mail-hantong-cli'))
. (Join-Path $PSScriptRoot 'package-common.ps1')
$script:InstallBase = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
$root = $script:InstallBase
Assert-SafePath $root
if (-not (Test-Path -LiteralPath $root)) { Write-Output 'CLI is not installed.'; exit 0 }
$marker = Get-ChildPath $root '.mail-hantong-owner'
if (-not (Test-Path -LiteralPath $marker)) { Write-Output 'No owned CLI installation found.'; exit 0 }
$lockPath = Get-ChildPath $root '.install.lock'
$lock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
try {
    # Preflight every version, ancestor, file hash and launcher before deletion.
    $state = Read-InstallState $root
    Remove-OwnedUserPath $root
    foreach ($version in $state.versions) {
        $directory = Get-ChildPath $root "versions/$($version.version)"
        Remove-OwnedFiles $directory $version.files
    }
    $launchers = @((Get-OwnedBinaries $state.versions) | ForEach-Object { "bin/$_.cmd" })
    foreach ($relative in ($launchers + @('current.txt', 'install-manifest.json', '.mail-hantong-owner'))) {
        $path = Get-ChildPath $root $relative
        if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path -Force }
    }
    foreach ($relative in @('bin', 'versions', 'staging')) {
        $path = Get-ChildPath $root $relative
        if ((Test-Path -LiteralPath $path -PathType Container) -and @(Get-ChildItem -LiteralPath $path -Force).Count -eq 0) {
            Remove-Item -LiteralPath $path -Force
        }
    }
} finally {
    $lock.Dispose()
}
Write-Output 'CLI files removed. Unknown files, mail data and keys were preserved. Close terminals to discard session-only PATH entries.'
