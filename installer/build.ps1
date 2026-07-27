$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$root = Split-Path -Parent $here

Write-Host 'building the daemon'
Push-Location $root
try { cargo build --release -p studiod; if ($LASTEXITCODE -ne 0) { throw 'studiod failed to build' } }
finally { Pop-Location }

Write-Host 'building the desktop shell'
Push-Location (Join-Path $root 'desktop')
try { cargo build --release; if ($LASTEXITCODE -ne 0) { throw 'the shell failed to build' } }
finally { Pop-Location }

$candidates = @(
  (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
  (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
  (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
)
$iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
  throw "Inno Setup 6 was not found. Install it with: winget install --id JRSoftware.InnoSetup"
}

Write-Host 'compiling the installer'
& $iscc (Join-Path $here 'game-studio-crew.iss')
if ($LASTEXITCODE -ne 0) { throw 'the installer failed to compile' }

Get-ChildItem (Join-Path $here 'out') -Filter *.exe |
  ForEach-Object { Write-Host ("built {0} ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB)) }
