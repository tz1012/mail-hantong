param([string] $Python)
$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Python) { $Python = (Get-Command python -ErrorAction SilentlyContinue).Source }
if (-not $Python) { throw 'Python 3.10 or newer is required.' }
& $Python (Join-Path $scriptRoot 'traceability.py') @args
exit $LASTEXITCODE
