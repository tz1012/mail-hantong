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
# The release manifest cannot hash itself. Keep its exact verified-source bytes
# as a separately owned installed file so first launch can verify sibling code.
try {
 $progressActivity = 'mail-hantong installation'
 $progressComplete = $false
    # BEGIN GENERATED INSTALL_PROGRESS_MESSAGES (from install-progress-en.json/install-progress-ko.json)
    $progressMessagesJson = @'
{"en":{"checking":"Checking system and release","selecting":"Selecting a compatible package","downloaded":"Downloaded asset {0} of {1}","verified":"Verified package checksum","extracted":"Extracted file {0} of {1}","installing":"Installing verified package","finalizing":"Finalizing installation","preparing":"Preparing installation","installed":"Installed file {0} of {1}","verified_file":"Verified file {0} of {1}","activating":"Activating installed version","complete":"Complete","activity":"mail-hantong installation"},"ko":{"checking":"\uc2dc\uc2a4\ud15c\uacfc \ub9b4\ub9ac\uc988 \ud655\uc778","selecting":"\ud638\ud658 \ud328\ud0a4\uc9c0 \uc120\ud0dd","downloaded":"\uc790\uc0b0 {0}/{1} \ub2e4\uc6b4\ub85c\ub4dc","verified":"\ud328\ud0a4\uc9c0 \uccb4\ud06c\uc12c \uac80\uc99d","extracted":"\ud30c\uc77c {0}/{1} \uc555\ucd95 \ud574\uc81c","installing":"\uac80\uc99d\ub41c \ud328\ud0a4\uc9c0 \uc124\uce58","finalizing":"\uc124\uce58 \ub9c8\ubb34\ub9ac","preparing":"\uc124\uce58 \uc900\ube44","installed":"\ud30c\uc77c {0}/{1} \uc124\uce58","verified_file":"\ud30c\uc77c {0}/{1} \uac80\uc99d","activating":"\uc124\uce58\ub41c \ubc84\uc804 \ud65c\uc131\ud654","complete":"\uc644\ub8cc","activity":"\ud55c\ud1b5 \uc124\uce58"}}
'@
    $progressMessages = $progressMessagesJson | ConvertFrom-Json
    function Get-InstallProgressText([string] $Key, [object[]] $Arguments) {
        $culture = if ($env:LANG) { $env:LANG } else { $PSUICulture }
        $language = if ($culture -match '^(?i:ko)([-_]|$)') { 'ko' } else { 'en' }
        $template = [string]$progressMessages.$language.PSObject.Properties[$Key].Value
        if ($Arguments.Count) { return ($template -f $Arguments) }
        return $template
    }
    # END GENERATED INSTALL_PROGRESS_MESSAGES
    $progressActivity = Get-InstallProgressText 'activity' @()
 function Set-InstallProgress([int] $Percent, [string] $Status) {
     $bounded = [Math]::Max(0, [Math]::Min(99, $Percent))
     Write-Progress -Id 2 -ParentId 1 -Activity $progressActivity -Status $Status -PercentComplete $bounded
 }
 function Complete-InstallProgress {
     Write-Progress -Id 2 -ParentId 1 -Activity $progressActivity -Status (Get-InstallProgressText 'complete' @()) -PercentComplete 100 -Completed
 }
 Set-InstallProgress 48 (Get-InstallProgressText 'checking' @())
$releaseManifestPath = Get-ChildPath $sourceFull 'release-manifest.json'
if (@($release.files | Where-Object { [string]$_.path -ieq 'release-manifest.json' }).Count -ne 0) { throw 'Release manifest cannot include itself' }
$ownedFiles = @($release.files) + @([pscustomobject]@{
    path = 'release-manifest.json'
    sha256 = (Get-FileHash -LiteralPath $releaseManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    size = (Get-Item -LiteralPath $releaseManifestPath).Length
})
Assert-FileRecords $ownedFiles
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

# TESTABLE_CORE_STOP_FUNCTIONS_BEGIN
function Get-CurrentUserSid {
    return ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value
}
function Get-CoreProcessRecords {
    return @(Get-CimInstance Win32_Process -Filter "Name = 'mail-hantong-core.exe'" |
        ForEach-Object {
            $owner = Invoke-CimMethod -InputObject $_ -MethodName GetOwner
            $sid = $null
            if ($owner.ReturnValue -eq 0) {
                try { $sid = ([Security.Principal.NTAccount]"$($owner.Domain)\$($owner.User)").Translate([Security.Principal.SecurityIdentifier]).Value } catch { $sid = $null }
            }
            [pscustomobject]@{ Pid = [int]$_.ProcessId; Path = [string]$_.ExecutablePath; CommandLine = [string]$_.CommandLine; OwnerSid = $sid }
        })
}
function Stop-CoreProcess([int] $ProcessId) {
    Stop-Process -Id $ProcessId -Force -ErrorAction Stop
}
function Assert-OwnedCoreFile([string] $Path, $Record) {
    Assert-SafePath $Path
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        -not ($item.PSIsContainer -eq $false) -or $item.Length -ne [int64]$Record.size) { throw 'Owned Core runtime is not a regular verified file' }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine [string]$Record.sha256) { throw 'Owned Core runtime hash changed' }
}
function Get-NormalizedWindowsPath([string] $Path) {
    $normalized = $Path.Replace('/', '\')
    if ($normalized.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
        $normalized = '\\' + $normalized.Substring(8)
    } elseif ($normalized.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) {
        $normalized = $normalized.Substring(4)
    }
    if ($normalized.StartsWith('\\', [StringComparison]::Ordinal)) {
        return '\\' + (($normalized.Substring(2)) -replace '\\{2,}', '\')
    }
    return $normalized -replace '\\{2,}', '\'
}
function Get-CanonicalWindowsPath([string] $Path) {
    return (Get-NormalizedWindowsPath ([IO.Path]::GetFullPath($Path)))
}
function Test-CoreCommandLine([string] $CommandLine, [string] $ExecutablePath) {
    $CommandLine = $CommandLine.Trim()
    $argsPattern = '(?:run(?:\s+--lang\s+(?:en|ko))?|--lang\s+(?:en|ko)\s+run)'
    if ($CommandLine -match "^$argsPattern$") { return $true }
    if ($CommandLine -match '^(?:"([^"]+)"|(\S+))\s+(.+)$') {
        $prefix = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
        return ([StringComparer]::OrdinalIgnoreCase.Equals((Get-CanonicalWindowsPath $prefix), (Get-CanonicalWindowsPath $ExecutablePath)) -and $Matches[3] -match "^$argsPattern$")
    }
    return $false
}
function Stop-OwnedCoreForUpdate([string] $Root, $Versions) {
    $currentSid = Get-CurrentUserSid
    $owned = @{}
    foreach ($version in $Versions) {
        $core = @($version.files | Where-Object { [string]$_.path -ieq 'bin/mail-hantong-core.exe' })
        $manifest = @($version.files | Where-Object { [string]$_.path -ieq 'release-manifest.json' })
        if ($core.Count -ne 1 -or $manifest.Count -gt 1) { throw 'Cannot verify installed Core runtime ownership' }
        $corePath = Get-ChildPath $Root "versions/$($version.version)/bin/mail-hantong-core.exe"
        Assert-OwnedCoreFile $corePath $core[0]
        # Older owned installations predate copying release-manifest.json.
        # Validate that sibling when present, but retain the legacy upgrade path.
        if ($manifest.Count -eq 1) {
            $manifestPath = Get-ChildPath $Root "versions/$($version.version)/release-manifest.json"
            Assert-OwnedCoreFile $manifestPath $manifest[0]
        }
        $owned[(Get-CanonicalWindowsPath $corePath)] = $true
    }
    $records = @(Get-CoreProcessRecords)
    $candidates = @()
    foreach ($record in $records) {
        if ([string]::IsNullOrWhiteSpace($record.Path)) { throw 'Cannot verify Core executable path' }
        $path = Get-CanonicalWindowsPath ([string]$record.Path)
        if ($owned.ContainsKey($path)) {
            if (-not (Test-CoreCommandLine ([string]$record.CommandLine) $path)) { throw 'Refusing to stop Core with an unsupported command line' }
            if ([string]$record.OwnerSid -ne $currentSid) { throw 'Refusing to stop Core owned by another user' }
            $candidates += [pscustomobject]@{ Pid = [int]$record.Pid; Path = $path; CommandLine = [string]$record.CommandLine; OwnerSid = [string]$record.OwnerSid }
        }
    }
    if ($candidates.Count -eq 0) { return }
    if ($candidates.Count -ne 1) { throw 'Multiple installed Core runtimes are running; refusing to stop an ambiguous process' }
    $candidate = $candidates[0]
    $fresh = @(Get-CoreProcessRecords | Where-Object { [int]$_.Pid -eq $candidate.Pid })
    if ($fresh.Count -ne 1 -or -not [StringComparer]::OrdinalIgnoreCase.Equals((Get-CanonicalWindowsPath ([string]$fresh[0].Path)), $candidate.Path) -or
        [string]$fresh[0].CommandLine -cne $candidate.CommandLine -or [string]$fresh[0].OwnerSid -cne $currentSid) {
        throw 'Core process changed during installer validation; refusing to stop it'
    }
    Stop-CoreProcess $candidate.Pid
}
# TESTABLE_CORE_STOP_FUNCTIONS_END
try {
     Set-InstallProgress 55 (Get-InstallProgressText 'preparing' @())
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
    $fileIndex = 0
    if (Test-Path -LiteralPath $versionDir) {
        if ($existing.Count -ne 1 -or $existing[0].files.Count -ne $ownedFiles.Count) { throw 'Existing version is unmanaged or differs' }
        $old = @{}
        foreach ($file in $existing[0].files) { $old[$file.path.Replace('/', '\')] = $file.sha256 }
        foreach ($file in $ownedFiles) {
            if ($old[$file.path.Replace('/', '\')] -ine $file.sha256) { throw 'Existing version differs' }
            Assert-FileHash (Get-ChildPath $versionDir $file.path) $file.sha256
            $fileIndex++
            Set-InstallProgress (55 + [int](25 * $fileIndex / $ownedFiles.Count)) (Get-InstallProgressText 'verified_file' @($fileIndex, $ownedFiles.Count))
        }
    } else {
        $stage = Get-ChildPath $root "staging/$([guid]::NewGuid().ToString('N'))"
        $null = New-Item -ItemType Directory -Force -Path $stage
        foreach ($file in $ownedFiles) {
            $destination = Get-ChildPath $stage $file.path
            $null = New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($destination))
            [IO.File]::Copy((Get-ChildPath $sourceFull $file.path), $destination, $false)
            $createdFiles += $file
            Assert-FileHash $destination $file.sha256
            $fileIndex++
            $copyPercent = 55 + [int](25 * $fileIndex / $ownedFiles.Count)
            Set-InstallProgress $copyPercent (Get-InstallProgressText 'installed' @($fileIndex, $ownedFiles.Count))
        }
        $null = New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($versionDir))
        Assert-SafePath $versionDir
        [IO.Directory]::Move($stage, $versionDir)
        $stage = $null
        $promoted = $true
        $versions += [pscustomobject]@{ version = $release.version; files = @($ownedFiles) }
    }
    $activeVersion = $null
    $currentPath = Get-ChildPath $root 'current.txt'
    if (Test-Path -LiteralPath $currentPath -PathType Leaf) { $activeVersion = [IO.File]::ReadAllText($currentPath) }
    if (-not $firstInstall -and $activeVersion -cne [string]$release.version) {
        Stop-OwnedCoreForUpdate $root $versions
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
    Write-AtomicText (Get-ChildPath $root 'current.txt') ([string]$release.version)
    Set-InstallProgress 92 (Get-InstallProgressText 'activating' @())
} catch {
    if (-not $recorded) {
        foreach ($shim in $createdShims) { Assert-SafePath $shim; Remove-Item -LiteralPath $shim -Force }
    }
    if ($stage -and (Test-Path -LiteralPath $stage)) { Remove-OwnedFiles $stage $createdFiles }
    if ($promoted -and -not $recorded) { Remove-OwnedFiles $versionDir $ownedFiles }
    if ($firstInstall -and -not $recorded) {
        foreach ($relative in @('bin/hantong.cmd', 'bin/mail-hantong-core.cmd', 'bin/update.cmd', '.mail-hantong-owner')) {
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
# Persist PATH only after activation succeeds. A locked activation pointer must
# leave the user's environment unchanged; a PATH failure can be retried safely.
try {
    if ($AddToPath) { Add-OwnedUserPath $root }
    $escapedBin = $bin.Replace("'", "''")
    Write-Output "Installed mail-hantong CLI $($release.version)."
    Write-Output "Start guided setup: & '$($versionDir.Replace("'", "''"))\bin\hantong.exe' init"
    Write-Output "For this PowerShell session: `$env:Path = '$escapedBin;' + `$env:Path"
    Write-Output "Uninstall with: & '$($versionDir.Replace("'", "''"))\uninstall.ps1' -InstallRoot '$($root.Replace("'", "''"))'"
    if ($AddToPath) { Write-Output 'User PATH is configured. Restart existing terminals to reload their environment.' }
    else { Write-Output 'No persistent PATH settings were changed.' }
    Write-Output 'No startup, account or mail-data settings were changed.'
    Complete-InstallProgress
    $progressComplete = $true
} catch {
    throw
}
finally {
    if (-not $progressComplete) { Write-Progress -Id 2 -Activity $progressActivity -Completed }
}
}
finally {
    if (-not $progressComplete) { Write-Progress -Id 2 -Activity 'mail-hantong installation' -Completed }
}
