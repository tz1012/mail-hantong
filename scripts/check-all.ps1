param(
    [string] $Python
)

$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot

function Test-PythonRuntime([string] $Command, [string[]] $Arguments) {
    try {
        $probe = 'import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)'
        & $Command @Arguments -c $probe 2>$null 1>$null
        return ($LASTEXITCODE -eq 0)
    } catch {
        return $false
    }
}

if ($Python) {
    if (-not (Test-PythonRuntime $Python @())) {
        Write-Error "The explicitly selected Python runtime is unavailable or older than Python 3.10: $Python"
        exit 127
    }
} else {
    $candidates = @()
    $pythonCommand = Get-Command python -ErrorAction SilentlyContinue
    if ($pythonCommand) { $candidates += ,@($pythonCommand.Source, @()) }
    $python3Command = Get-Command python3 -ErrorAction SilentlyContinue
    if ($python3Command) { $candidates += ,@($python3Command.Source, @()) }
    $pyCommand = Get-Command py -ErrorAction SilentlyContinue
    if ($pyCommand) { $candidates += ,@($pyCommand.Source, @('-3')) }
    foreach ($candidate in $candidates) {
        if (Test-PythonRuntime $candidate[0] $candidate[1]) {
            $Python = $candidate[0]
            $pythonArguments = $candidate[1]
            break
        }
    }
}
if (-not $Python) {
    Write-Error 'Python 3.10 or newer is required to run check_all.py.'
    exit 127
}

& $Python @pythonArguments (Join-Path $scriptRoot 'check_all.py')
exit $LASTEXITCODE
