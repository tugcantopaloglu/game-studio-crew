<#
.SYNOPSIS
  Game Studio Crew installer for Windows.

.DESCRIPTION
  irm https://raw.githubusercontent.com/tugcantopaloglu/game-studio-crew/main/scripts/install.ps1 | iex

  Downloads the signed-nothing, per-user setup from the latest GitHub release
  and runs it. No administrator rights are needed and PATH is left alone.

.PARAMETER Version
  Which release to fetch. Defaults to the latest.

.PARAMETER Portable
  Unpack the zip into %LOCALAPPDATA% instead of running the installer.
#>
[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [switch]$Portable
)

$ErrorActionPreference = 'Stop'
$repo = 'tugcantopaloglu/game-studio-crew'

function Fail($message) {
    Write-Host "error: $message" -ForegroundColor Red
    exit 1
}

Write-Host 'Game Studio Crew installer'

if ([Environment]::Is64BitOperatingSystem -eq $false) {
    Fail 'this build is 64-bit only'
}

$api = if ($Version -eq 'latest') {
    "https://api.github.com/repos/$repo/releases/latest"
} else {
    "https://api.github.com/repos/$repo/releases/tags/$Version"
}

try {
    $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'game-studio-crew-installer' }
} catch {
    Fail "could not reach the GitHub release API: $($_.Exception.Message)"
}

$wanted = if ($Portable) { '*windows-x86_64.zip' } else { '*-setup.exe' }
$asset = $release.assets | Where-Object { $_.name -like $wanted } | Select-Object -First 1

if (-not $asset -and -not $Portable) {
    Write-Host '  no setup.exe in this release; falling back to the portable zip'
    $Portable = $true
    $asset = $release.assets | Where-Object { $_.name -like '*windows-x86_64.zip' } | Select-Object -First 1
}
if (-not $asset) { Fail "no Windows build in the $Version release of $repo" }

$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("gsc-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
$download = Join-Path $temp $asset.name

Write-Host "  fetching: $($asset.name)"
try {
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $download -UseBasicParsing
} catch {
    Fail "download failed: $($_.Exception.Message)"
}

if ($Portable) {
    $dest = Join-Path $env:LOCALAPPDATA 'Programs\Game Studio Crew'
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    New-Item -ItemType Directory -Path $dest -Force | Out-Null

    Expand-Archive -Path $download -DestinationPath $temp -Force
    $unpacked = Get-ChildItem -Path $temp -Directory | Select-Object -First 1
    Copy-Item -Path (Join-Path $unpacked.FullName '*') -Destination $dest -Recurse -Force

    Write-Host ''
    Write-Host "Installed to $dest"
    Write-Host "Run '$dest\game-studio.exe' to open the studio."
} else {
    Write-Host '  running the installer'
    $run = Start-Process -FilePath $download -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
    if ($run.ExitCode -ne 0) { Fail "the installer exited with code $($run.ExitCode)" }

    Write-Host ''
    Write-Host 'Installed. Game Studio Crew is in the Start Menu.'
}

Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue

Write-Host "Run 'studiod doctor' to see what the studio still needs."
