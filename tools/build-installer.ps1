# Builds the NSIS installer from a clean checkout and copies it outside the repo.
# Steps: sidecar (Go) -> npm ci -> tauri build -> copy to ..\dist (git-ignored).
# Usage (PowerShell, repo root): powershell -ExecutionPolicy Bypass -File .\tools\build-installer.ps1 [-OutDir <folder>]
param([string]$OutDir = (Join-Path $PSScriptRoot '..\dist'))
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) { $env:PATH += ";$cargoBin" }

function Step($name, [scriptblock]$body) {
    Write-Host "==> $name"
    & $body
    if ($LASTEXITCODE -ne 0 -and $null -ne $LASTEXITCODE) { throw "$name failed (exit $LASTEXITCODE)" }
}

Step 'Build sidecar' { & (Join-Path $repo 'tools\r6-dissect\build.ps1') }

Push-Location (Join-Path $repo 'app')
try {
    Step 'npm ci' { npm ci }
    Step 'tauri build' { npm run tauri build }
} finally { Pop-Location }

$conf = Get-Content (Join-Path $repo 'app\src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$version = $conf.version
# Cargo workspace target dir is at the repo root; fall back to the app one.
$bundleDirs = @('target\release\bundle\nsis', 'app\src-tauri\target\release\bundle\nsis') | ForEach-Object { Join-Path $repo $_ }
$found = $bundleDirs | Where-Object { Test-Path $_ } | ForEach-Object {
    Get-ChildItem $_ -Filter "*_${version}_*setup.exe" } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $found) { throw "NSIS installer for version $version not found" }

New-Item -ItemType Directory -Force $OutDir | Out-Null
$dest = Join-Path $OutDir "R6-Coach-Setup-$version.exe"
Copy-Item $found.FullName $dest -Force
$item = Get-Item $dest
Write-Host ("Installer: {0}" -f $item.FullName)
Write-Host ("Size: {0:N0} bytes ({1:N1} MB)" -f $item.Length, ($item.Length / 1MB))
Write-Host 'Unsigned: Windows SmartScreen will warn. Click "More info" -> "Run anyway".'
