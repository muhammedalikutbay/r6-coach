# Builds the patched r6-dissect as a Tauri sidecar binary.
# Output: app/src-tauri/binaries/r6-dissect-x86_64-pc-windows-msvc.exe (not committed).
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Resolve-Path (Join-Path $here '..\..')
$outDir = Join-Path $repo 'app\src-tauri\binaries'
New-Item -ItemType Directory -Force $outDir | Out-Null
$out = Join-Path $outDir 'r6-dissect-x86_64-pc-windows-msvc.exe'

$goBin = 'C:\Program Files\Go\bin'
if ((Test-Path $goBin) -and ($env:PATH -notlike "*$goBin*")) { $env:PATH += ";$goBin" }

Push-Location $here
try {
    $env:GOOS = 'windows'; $env:GOARCH = 'amd64'
    go build -trimpath -ldflags "-s -w -X main.Version=r6koc-patched" -o $out .
    if ($LASTEXITCODE -ne 0) { throw "go build failed ($LASTEXITCODE)" }
} finally { Pop-Location }
Write-Host "Built $out ($([math]::Round((Get-Item $out).Length/1MB,1)) MB)"
