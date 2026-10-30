#requires -Version 5.1
[CmdletBinding()]
param([string] $Version = 'latest', [string] $InstallRoot, [switch] $SessionOnly)

function Install-MailHantong {
    param([string] $ReleaseVersion = 'latest', [string] $Destination, [switch] $SessionOnly)
    $ErrorActionPreference = 'Stop'
    if ($ReleaseVersion -ne 'latest' -and $ReleaseVersion -notmatch '^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$') {
        throw 'Invalid release version.'
    }
    if (-not [Environment]::Is64BitOperatingSystem -or [Environment]::OSVersion.Platform -ne 'Win32NT') {
        throw 'This package requires 64-bit Windows.'
    }
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $headers = @{ 'User-Agent' = 'mail-hantong-installer'; 'Accept' = 'application/vnd.github+json' }
    $base = 'https://api.github.com/repos/tz1012/mail-hantong/releases'
    if ($ReleaseVersion -eq 'latest') {
        $releases = @(Invoke-RestMethod -Uri "$base`?per_page=20" -Headers $headers)
    } else {
        $tag = if ($ReleaseVersion.StartsWith('v')) { $ReleaseVersion } else { "v$ReleaseVersion" }
        $releases = @(Invoke-RestMethod -Uri "$base/tags/$tag" -Headers $headers)
    }
    $selected = $null
    foreach ($release in $releases) {
        if ($release.draft -or $release.tag_name -notmatch '^v(?<label>[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?)$') { continue }
        $label = $Matches.label
        $name = "mail-hantong-cli-$label-windows-x86_64.zip"
        $zip = @($release.assets | Where-Object { $_.name -ceq $name })
        $sums = @($release.assets | Where-Object { $_.name -ceq 'SHA256SUMS' })
        if ($zip.Count -eq 1 -and $sums.Count -eq 1 -and $zip[0].size -gt 0 -and $zip[0].size -le 134217728 -and $sums[0].size -le 65536) {
            $selected = @{ Zip = $zip[0]; Sums = $sums[0]; Name = $name; Label = $label }
            break
        }
    }
    if (-not $selected) { throw 'No compatible published Windows package is available.' }
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('mail-hantong-install-' + [guid]::NewGuid().ToString('N'))
    $temporary = [IO.Path]::GetFullPath($temporary)
    $tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $temporary.StartsWith($tempBase, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid temporary directory.' }
    $null = New-Item -ItemType Directory -Path $temporary
    try {
        foreach ($asset in @($selected.Zip, $selected.Sums)) {
            $uri = [Uri]$asset.browser_download_url
            $prefix = '/tz1012/mail-hantong/releases/download/'
            if ($uri.Scheme -cne 'https' -or $uri.Host -cne 'github.com' -or -not $uri.AbsolutePath.StartsWith($prefix, [StringComparison]::Ordinal)) {
                throw 'Unexpected release asset URL.'
            }
            $output = Join-Path $temporary $asset.name
            Invoke-WebRequest -Uri $uri.AbsoluteUri -UseBasicParsing -Headers $headers -OutFile $output
            if ((Get-Item -LiteralPath $output).Length -ne $asset.size) { throw 'Downloaded asset size mismatch.' }
        }
        $expected = $null
        foreach ($line in [IO.File]::ReadAllLines((Join-Path $temporary 'SHA256SUMS'))) {
            if ($line -match '^([0-9a-fA-F]{64})  (.+)$' -and $Matches[2] -ceq $selected.Name) {
                if ($expected) { throw 'Duplicate package checksum.' }
                $expected = $Matches[1]
            }
        }
        $archive = Join-Path $temporary $selected.Name
        if (-not $expected -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ine $expected) { throw 'Package checksum mismatch.' }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $destinationRoot = Join-Path $temporary 'package'
        $null = New-Item -ItemType Directory -Path $destinationRoot
        $boundary = [IO.Path]::GetFullPath($destinationRoot).TrimEnd('\') + '\'
        $reader = [IO.Compression.ZipFile]::OpenRead($archive)
        try {
            if ($reader.Entries.Count -gt 2048) { throw 'Package contains too many entries.' }
            $seen = @{}
            [long] $total = 0
            foreach ($entry in $reader.Entries) {
                $relative = $entry.FullName.Replace('/', '\')
                $parts = $relative.Split('\')
                if ($parts.Count -eq 0 -or @($parts | Where-Object { $_ -in @('', '.', '..') -or $_ -match '[<>:"|?*\x00-\x1f]' -or $_ -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)' -or $_.EndsWith('.') -or $_.EndsWith(' ') }).Count -ne 0) { throw 'Invalid archive path.' }
                $path = [IO.Path]::GetFullPath((Join-Path $destinationRoot $relative))
                if (-not $path.StartsWith($boundary, [StringComparison]::OrdinalIgnoreCase) -or $seen.ContainsKey($path)) { throw 'Duplicate or escaping archive path.' }
                if (($entry.ExternalAttributes -band 0xF0000000L) -eq 0xA0000000L) { throw 'Archive links are unsupported.' }
                $seen[$path] = $true
                $total += $entry.Length
                if ($entry.Length -gt 67108864 -or $total -gt 268435456) { throw 'Extracted package exceeds its limit.' }
                $null = New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($path))
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $path, $false)
            }
        } finally { $reader.Dispose() }
        $installer = Join-Path $destinationRoot 'install.ps1'
        if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw 'Package installer is missing.' }
        if ([string]::IsNullOrWhiteSpace($Destination)) { $Destination = Join-Path $env:LOCALAPPDATA 'Programs\mail-hantong-cli' }
        & ([ScriptBlock]::Create([IO.File]::ReadAllText($installer))) -PackageRoot $destinationRoot -Source $destinationRoot -InstallRoot $Destination -AddToPath:(-not $SessionOnly)
        $env:Path = (Join-Path ([IO.Path]::GetFullPath($Destination)) 'bin') + ';' + $env:Path
        Write-Output "mail-hantong $($selected.Label) is ready in this terminal. Run: hantong --help"
    } finally {
        # Only the unique directory created above is removed; never the program
        # directory, existing profiles, mail data, or a caller-supplied path.
        if ($temporary.StartsWith($tempBase, [StringComparison]::OrdinalIgnoreCase) -and (Test-Path -LiteralPath $temporary)) {
            Remove-Item -LiteralPath $temporary -Recurse -Force
        }
    }
}

if ($MyInvocation.InvocationName -ne '.') { Install-MailHantong -ReleaseVersion $Version -Destination $InstallRoot -SessionOnly:$SessionOnly }
