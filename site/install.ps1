# Cardboard installer for Windows.
#   irm https://cardboard.goerll.dev/install.ps1 | iex
# Re-run it to update. Set $env:CARDBOARD_VERSION = "v0.1.0" to pin a release.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repo = "goerll/cardboard"
$version = if ($env:CARDBOARD_VERSION) { $env:CARDBOARD_VERSION } else { "latest" }
if (-not [Environment]::Is64BitOperatingSystem) { throw "Cardboard needs 64-bit Windows." }
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { Write-Host "No native ARM64 build yet, installing x64 (runs under emulation)." }

$asset = "cardboard-windows-x64.zip"
$url = if ($version -eq "latest") {
  "https://github.com/$repo/releases/latest/download/$asset"
} else {
  "https://github.com/$repo/releases/download/$version/$asset"
}

$dest = Join-Path $env:LOCALAPPDATA "Programs\Cardboard"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("cardboard-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
  Write-Host "Downloading $asset"
  Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $asset) -UseBasicParsing
  Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force

  Get-Process cardboard-desktop, cardboard -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dest*" } | Stop-Process -Force
  if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
  New-Item -ItemType Directory -Path (Split-Path $dest) -Force | Out-Null
  Move-Item (Join-Path $tmp "cardboard-windows-x64") $dest
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$programs = [Environment]::GetFolderPath("Programs")
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut((Join-Path $programs "Cardboard.lnk"))
$link.TargetPath = Join-Path $dest "cardboard-desktop.exe"
$link.WorkingDirectory = $dest
$link.IconLocation = Join-Path $dest "cardboard.ico"
$link.Description = "Local-first HTML/CSS design editor"
$link.Save()

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not (($userPath -split ";") -contains $dest)) {
  [Environment]::SetEnvironmentVariable("Path", (($userPath.TrimEnd(";"), $dest) -join ";").TrimStart(";"), "User")
  $env:Path = "$env:Path;$dest"
}

Write-Host "Cardboard installed. Open it from the Start menu."
Write-Host "Agent setup (MCP): point your agent at `"$dest\cardboard.exe`" with args [`"mcp`"]."
