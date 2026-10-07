# Install / upgrade harl on Windows.
#
#   irm https://github.com/Jannchie/harl/releases/latest/download/install.ps1 | iex
#
# To also register it to start at login, set HARL_SERVICE=serve (dashboard) or agent (collect only)
# before running the line above, or: & ([scriptblock]::Create((irm <url above>))) -Service agent
#
# Kept ASCII-only on purpose: Windows PowerShell 5.1 reads BOM-less scripts as ANSI.
param(
  [ValidateSet('', 'serve', 'agent')]
  [string]$Service = $env:HARL_SERVICE,
  [string]$Version = 'latest',
  [string]$InstallDir = (Join-Path $HOME '.harl\bin')
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'Jannchie/harl'
$target = 'x86_64-pc-windows-msvc'
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') {
  Write-Warning "No build for $env:PROCESSOR_ARCHITECTURE; installing x86_64 (runs under emulation on Windows on ARM)"
}
$asset = "harl-$target.zip"
$base = if ($Version -eq 'latest') { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/v$($Version.TrimStart('v'))" }

$harl = Join-Path $InstallDir 'harl.exe'
$wasRunning = $false
$tmp = Join-Path ([IO.Path]::GetTempPath()) "harl-install-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
  Write-Host "Downloading $base/$asset"
  Invoke-WebRequest "$base/$asset" -OutFile (Join-Path $tmp $asset) -UseBasicParsing
  Invoke-WebRequest "$base/$asset.sha256" -OutFile (Join-Path $tmp "$asset.sha256") -UseBasicParsing
  $want = ((Get-Content (Join-Path $tmp "$asset.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
  $got = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
  if ($want -ne $got) { throw "sha256 mismatch: $got != $want" }
  Expand-Archive (Join-Path $tmp $asset) -DestinationPath (Join-Path $tmp 'x') -Force

  # A running background service keeps the exe busy: stop it first, start it again afterwards
  $harlw = Join-Path $InstallDir 'harlw.exe'
  $wasRunning = [bool](Get-Process harlw -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $harlw })
  if ($wasRunning) { & $harl service stop | Out-Null }
  New-Item -ItemType Directory -Force $InstallDir | Out-Null
  foreach ($exe in 'harl.exe', 'harlw.exe') {
    $src = Get-ChildItem (Join-Path $tmp 'x') -Recurse -Filter $exe | Select-Object -First 1
    if (-not $src) { throw "$exe not found in the archive" }
    $dst = Join-Path $InstallDir $exe
    if (Test-Path $dst) { Move-Item $dst "$dst.old" -Force }
    Copy-Item $src.FullName $dst -Force
  }
} finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $InstallDir) {
  [Environment]::SetEnvironmentVariable('Path', (@($userPath, $InstallDir) | Where-Object { $_ }) -join ';', 'User')
  $env:Path = "$env:Path;$InstallDir"
  Write-Host "Added $InstallDir to your user PATH (new terminals pick it up)"
}

& $harl --version
if ($Service) {
  if ($Service -eq 'agent') { & $harl service install --agent } else { & $harl service install }
} elseif ($wasRunning) {
  & $harl service start
} else {
  Write-Host "Start at login: harl service install (dashboard) or harl service install --agent (collect only)"
}
