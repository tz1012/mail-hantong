param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('up', 'down', 'status')]
    [string] $Action
)

$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot
$compose = Join-Path $repoRoot 'tests/Integration/imap/compose.yaml'

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Error 'Docker is required for the development-only test mail server.'
    exit 127
}
if (-not (Test-Path -LiteralPath $compose -PathType Leaf)) {
    Write-Error "Compose file is missing: $compose"
    exit 2
}

$composeArgs = @('compose', '-f', $compose)
if ($Action -eq 'up') {
    & docker @composeArgs up -d --wait
} elseif ($Action -eq 'down') {
    & docker @composeArgs down --remove-orphans
} else {
    & docker @composeArgs ps
}
exit $LASTEXITCODE
