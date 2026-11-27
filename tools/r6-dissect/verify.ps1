# Runs r6-dissect over every .rec round file and every match folder, then reports counts.
# Usage: ./verify.ps1 [-Exe <path>] [-DataDir <MatchReplay dir>] [-ReferenceExe <upstream exe>]
# With -ReferenceExe, rounds the reference reads successfully must produce identical JSON
# (apart from the unknown-ID name fallback, which is only compared when the reference succeeded).
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\app\src-tauri\binaries\r6-dissect-x86_64-pc-windows-msvc.exe'),
    [string]$DataDir = $env:R6K_REPLAY_DIR,
    [string]$ReferenceExe = ''
)
$ErrorActionPreference = 'Stop'
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("r6verify-" + [guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null

function Invoke-Dissect([string]$exe, [string]$inPath, [string]$outFile) {
    $ErrorActionPreference = 'Continue'  # native stderr must not abort in PS 5.1
    if (Test-Path $outFile) { Remove-Item $outFile }
    & $exe $inPath -o $outFile 2>$null | Out-Null
    $ok = ($LASTEXITCODE -eq 0) -and (Test-Path $outFile) -and ((Get-Item $outFile).Length -gt 2)
    if ($ok) { try { $null = Get-Content $outFile -Raw | ConvertFrom-Json } catch { $ok = $false } }
    return $ok
}

$rounds = Get-ChildItem $DataDir -Recurse -Filter *.rec
$folders = Get-ChildItem $DataDir -Directory
$rOk = 0; $rFail = @(); $same = 0; $diff = @(); $refOk = 0
$i = 0
foreach ($f in $rounds) {
    $i++
    $out = Join-Path $tmp "r$i.json"
    if (Invoke-Dissect $Exe $f.FullName $out) { $rOk++ } else { $rFail += $f.Name; continue }
    if ($ReferenceExe) {
        $ref = Join-Path $tmp "ref$i.json"
        if (Invoke-Dissect $ReferenceExe $f.FullName $ref) {
            $refOk++
            if ((Get-Content $out -Raw) -eq (Get-Content $ref -Raw)) { $same++ } else { $diff += $f.Name }
        }
    }
}
$mOk = 0; $mFail = @()
foreach ($d in $folders) {
    $out = Join-Path $tmp ("m" + $d.Name + ".json")
    if (Invoke-Dissect $Exe $d.FullName $out) { $mOk++ } else { $mFail += $d.Name }
}
Write-Host "Rounds : $rOk / $($rounds.Count) ok"
Write-Host "Matches: $mOk / $($folders.Count) ok"
if ($ReferenceExe) { Write-Host "Reference-ok rounds: $refOk, identical output: $same, different: $($diff.Count)" }
if ($rFail) { Write-Host "Failed rounds:`n  $($rFail -join "`n  ")" }
if ($mFail) { Write-Host "Failed matches:`n  $($mFail -join "`n  ")" }
if ($diff) { Write-Host "Differing rounds:`n  $($diff -join "`n  ")" }
Remove-Item $tmp -Recurse -Force
if ($rFail -or $mFail) { exit 1 }
