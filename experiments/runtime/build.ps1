$ErrorActionPreference = 'Stop'
$experimentRoot = $PSScriptRoot
$workspaceRoot = (Resolve-Path (Join-Path $experimentRoot '../..')).Path
$toolRoot = Join-Path $workspaceRoot '.tools'
$sdkRoot = Join-Path $toolRoot 'dotnet10'
$dotnetExe = Join-Path $sdkRoot 'dotnet.exe'
$sdkVersion = '10.0.403'
$sdkUrl = 'https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.403/dotnet-sdk-10.0.403-win-x64.zip'
$sdkHash = '7a114c318fa7f1d9822991b534b2814d0e78386ceda816b371f5141acefe741d87ccb606ede06cadac8f9ac5070e32b708a67bbd1619ff5e5a7858a27874ef48'

if ($env:OS -ne 'Windows_NT' -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
    throw 'This experiment build script requires Windows x64.'
}
Get-Command cargo, rustc -ErrorAction Stop | Out-Null
New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null
if (-not (Test-Path -LiteralPath $dotnetExe)) {
    $archive = Join-Path $toolRoot "dotnet-sdk-$sdkVersion-win-x64.zip"
    if (-not (Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest -Uri $sdkUrl -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA512).Hash.ToLowerInvariant() -ne $sdkHash) {
        throw 'SDK archive checksum mismatch. No archive was executed.'
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $sdkRoot -Force
}
if ((& $dotnetExe --version) -ne $sdkVersion) { throw 'Unexpected local SDK version.' }

$environmentNames = @('DOTNET_ROOT', 'DOTNET_CLI_HOME', 'NUGET_PACKAGES', 'DOTNET_CLI_TELEMETRY_OPTOUT', 'DOTNET_NOLOGO')
$savedEnvironment = @{}
foreach ($name in $environmentNames) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    $env:DOTNET_ROOT = $sdkRoot
    $env:DOTNET_CLI_HOME = Join-Path $toolRoot 'dotnet-home'
    $env:NUGET_PACKAGES = Join-Path $toolRoot 'nuget'
    $env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
    $env:DOTNET_NOLOGO = '1'
    & cargo build --manifest-path (Join-Path $experimentRoot 'rust/Cargo.toml') --release --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Rust release build failed.' }
    & $dotnetExe publish (Join-Path $experimentRoot 'dotnet/hantong-runtime-dotnet.csproj') -r win-x64 -c Release --nologo -v minimal
    if ($LASTEXITCODE -ne 0) { throw 'Native AOT publish failed.' }
    $sources = @('SPEC.md', 'benchmark.py', 'test_contract.py', 'test_harness.py', 'build.ps1', 'rust/Cargo.toml', 'rust/Cargo.lock', 'rust/src/main.rs', 'dotnet/global.json', 'dotnet/hantong-runtime-dotnet.csproj', 'dotnet/Program.cs')
    $sourceHashes = [ordered]@{}
    foreach ($source in $sources) { $sourceHashes[$source] = (Get-FileHash -LiteralPath (Join-Path $experimentRoot $source) -Algorithm SHA256).Hash.ToLowerInvariant() }
    $manifest = [ordered]@{
        rustc = (& rustc -Vv) -join "`n"
        cargo = (& cargo -V)
        rust_profile = 'release'
        rust_options = 'opt-level=3; no PGO; no target-cpu=native'
        rust_build_command = 'cargo build --release --locked --offline'
        dotnet_sdk = (& $dotnetExe --version)
        dotnet_runtimes = (& $dotnetExe --list-runtimes) -join "`n"
        dotnet_native_aot = $true
        dotnet_options = 'PublishAot=true; OptimizationPreference=Speed; InvariantGlobalization=true; win-x64; Release'
        dotnet_build_command = 'dotnet publish -r win-x64 -c Release'
        sdk_url = $sdkUrl
        sdk_sha512 = $sdkHash
        source_sha256 = $sourceHashes
        storage = 'Not auto-detected by build script; see measurement report for verified volume information'
    }
    $resultsRoot = Join-Path $experimentRoot 'results'
    New-Item -ItemType Directory -Force -Path $resultsRoot | Out-Null
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $resultsRoot 'build-manifest.json') -Encoding utf8
} finally {
    foreach ($name in $environmentNames) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
}
