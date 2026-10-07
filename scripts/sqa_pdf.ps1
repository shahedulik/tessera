param(
    [string]$Root = 'D:\tessera',
    [string]$OutputRoot = 'D:\tessera\evidence\pdf_out',
    [switch]$AllowUnsafeOutputRoot
)

$ErrorActionPreference = 'Continue'

$tempRoot = 'D:\tessera-cache\temp'
if ($Root -ne 'D:\tessera') { $tempRoot = Join-Path $Root (Join-Path 'target' 'tmp') }
New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
$env:TEMP = $tempRoot
$env:TMP = $tempRoot

$exeSuffix = ''
if ($env:OS -eq 'Windows_NT') { $exeSuffix = '.exe' }
$exe = Join-Path $Root (Join-Path 'target' (Join-Path 'release' ("tessera_core" + $exeSuffix)))
$fixture = Join-Path $Root (Join-Path 'evidence' (Join-Path 'synthetic' 'tessera_fixture_1p.pdf'))
$sealedFixture = '96bdc602f391621565cf76ed362cecd53326fa5ca520a4c52f3291bd6a578909'

$results = @()

function Add-Result {
    param([string]$Step, [string]$Detail, [bool]$Pass)
    $script:results += [pscustomobject]@{
        Step   = $Step
        Result = $(if ($Pass) { 'PASS' } else { 'FAIL' })
        Detail = $Detail
    }
}

Write-Host 'TESSERA // SQA PDF-VEIN ACCEPTANCE RUNNER // v1.0 (P2A, CPU-only, no GPU/network)'
Write-Host ("Host: {0}  |  Root: {1}  |  TEMP routed: {2}  |  {3}" -f $env:COMPUTERNAME, $Root, $env:TEMP, (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ''
Set-Location $Root

Write-Host '[1/7] fixture seal'
$fixtureHash = (Get-FileHash $fixture -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "  $fixtureHash"
Add-Result 'fixture sha256' $fixtureHash.Substring(0, 16) ($fixtureHash -ceq $sealedFixture)

Write-Host '[2/7] clippy -p tessera_vision --features pdf --all-targets -- -D warnings'
cargo clippy -p tessera_vision --features pdf --all-targets -- -D warnings 2>&1 | Select-Object -Last 2 | ForEach-Object { Write-Host "  $_" }
Add-Result 'clippy (vision, pdf)' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[3/7] cargo test -p tessera_vision --features pdf (incl. 2x10k fuzz)'
cargo test -p tessera_vision --features pdf 2>&1 | Select-String 'test result' | ForEach-Object { Write-Host "  $_" }
Add-Result 'cargo test (pdf suite)' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[4/7] cargo build -p tessera_core --features pdf --release'
cargo build -p tessera_core --features pdf --release 2>&1 | Select-Object -Last 2 | ForEach-Object { Write-Host "  $_" }
Add-Result 'build (core, pdf, release)' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[5/7] pdfscan --dry-run (sealed fixture, writes nothing)'
$dryOut = & $exe pdfscan $fixture --dry-run 2>&1 | Out-String
$dryOut -split "`r?`n" | Where-Object { $_ -ne '' } | Select-Object -Last 3 | ForEach-Object { Write-Host "  $_" }
$dryOk = ($LASTEXITCODE -eq 0) -and ($dryOut -match 'dry-run: accessibility OK \| pages=1') -and ($dryOut -match 'verdict=PASS')
Add-Result 'pdfscan --dry-run' "exit=$LASTEXITCODE" $dryOk

Write-Host '[6/7] pdfscan full (sealed fixture)'
$scanArgs = @('pdfscan', $fixture, '--output-root', $OutputRoot)
if ($AllowUnsafeOutputRoot) { $scanArgs += '--allow-unsafe-output-root' }
$scanOut = & $exe @scanArgs 2>&1 | Out-String
$scanOut -split "`r?`n" | Where-Object { $_ -ne '' } | Select-Object -Last 6 | ForEach-Object { Write-Host "  $_" }
$scanOk = ($LASTEXITCODE -eq 0) -and ($scanOut -match 'PDFSCAN \| pages=1') -and ($scanOut -match 'verdict=PASS')
Add-Result 'pdfscan full' "exit=$LASTEXITCODE" $scanOk

Write-Host '[7/7] output manifest hash validation + zero-C: audit'
$prefix = $sealedFixture.Substring(0, 16)
$outDir = Join-Path $OutputRoot $prefix
if (Test-Path $outDir) {
    $manifestOk = $true
    $renderLines = @(Get-Content (Join-Path $outDir 'render_manifest.jsonl'))
    if ($renderLines.Count -lt 1) { $manifestOk = $false; Write-Host '  empty render_manifest.jsonl' }
    foreach ($line in $renderLines) {
        if ($line -match '"png_file": "([^"]+)", "image_sha256": "([0-9a-f]{64})"') {
            $png = Join-Path $outDir $Matches[1]
            if (-not (Test-Path $png)) { $manifestOk = $false; Write-Host "  MISSING PNG: $($Matches[1])"; continue }
            $actual = (Get-FileHash $png -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($actual -cne $Matches[2]) { $manifestOk = $false; Write-Host "  HASH MISMATCH: $($Matches[1])" }
        } else {
            $manifestOk = $false
            Write-Host "  UNPARSEABLE LINE: $line"
        }
    }
    $indexContent = Get-Content (Join-Path $outDir 'evidence_index.json') -Raw
    if ($indexContent -notmatch $sealedFixture) { $manifestOk = $false; Write-Host '  index missing source seal' }
    $textFiles = Get-ChildItem $outDir -File | Where-Object { $_.Extension -eq '.json' -or $_.Extension -eq '.jsonl' }
    $cLeak = $textFiles | Select-String -Pattern 'C:\\', 'C:/'
    if ($cLeak) { $manifestOk = $false; Write-Host '  C: PATH LEAK DETECTED IN OUTPUTS' }
    Add-Result 'output hashes + zero-C: audit' $outDir $manifestOk
} else {
    Add-Result 'output hashes + zero-C: audit' "missing $outDir" $false
}

Write-Host ''
$results | Format-Table -AutoSize | Out-Host
$failed = @($results | Where-Object { $_.Result -eq 'FAIL' }).Count
if ($failed -gt 0) {
    Write-Host ("SQA-PDF: FAIL ({0} of {1} steps failed). P2A NOT accepted." -f $failed, $results.Count)
    exit 1
}
Write-Host ("SQA-PDF: PASS ({0} of {1}). P2A PDF Evidence Vein accepted." -f $results.Count, $results.Count)
exit 0
