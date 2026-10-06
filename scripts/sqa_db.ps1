param([string]$Root = 'D:\tessera')

$ErrorActionPreference = 'Continue'

$exeSuffix = ''
if ($env:OS -eq 'Windows_NT') { $exeSuffix = '.exe' }
$exe = Join-Path $Root (Join-Path 'target' (Join-Path 'debug' ("tessera_core" + $exeSuffix)))
$fixture = Join-Path $Root (Join-Path 'evidence' (Join-Path 'synthetic' 'synthetic_tenders.csv'))
$sealedFixture = '2fe27f0f06c362361a407c34df53b3fc3abe9534effcf3e7a78c01421e61a8a4'

$results = @()

function Add-Result {
    param([string]$Step, [string]$Detail, [bool]$Pass)
    $script:results += [pscustomobject]@{
        Step   = $Step
        Result = $(if ($Pass) { 'PASS' } else { 'FAIL' })
        Detail = $Detail
    }
}

Write-Host 'TESSERA // SQA DB ACCEPTANCE RUNNER // v1.0 (P1-002 ingest + P1-003 rings)'
Write-Host ("Host: {0}  |  Root: {1}  |  {2}" -f $env:COMPUTERNAME, $Root, (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ''

Set-Location $Root

Write-Host '[1/6] environment contract (KUZU_LIBRARY_DIR)'
$kuzuOk = -not [string]::IsNullOrEmpty($env:KUZU_LIBRARY_DIR)
Add-Result 'KUZU_LIBRARY_DIR set' (": $(if ($kuzuOk) { $env:KUZU_LIBRARY_DIR } else { '(not set)' })") $kuzuOk

Write-Host '[2/6] cargo build -p tessera_core --features db'
cargo build -p tessera_core --features db 2>&1 | Select-Object -Last 2 | ForEach-Object { Write-Host "  $_" }
Add-Result 'build (db feature)' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[3/6] fixture seal'
$fixtureHash = (Get-FileHash $fixture -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "  $fixtureHash"
Add-Result 'fixture sha256' $fixtureHash.Substring(0, 16) ($fixtureHash -ceq $sealedFixture)

Write-Host '[4/6] ingest acceptance (rows=5000, custody, verdict)'
$ingestOut = & $exe ingest $fixture 2>&1 | Out-String
$ingestOut -split "`r?`n" | Where-Object { $_ -ne '' } | Select-Object -Last 4 | ForEach-Object { Write-Host "  $_" }
$ingestOk = ($LASTEXITCODE -eq 0) -and ($ingestOut -match 'chain-of-custody: PASS') -and ($ingestOut -match 'rows=5000') -and ($ingestOut -match 'verdict=PASS')
Add-Result 'ingest P1-002' "exit=$LASTEXITCODE" $ingestOk

Write-Host '[5/6] rings acceptance (RING-01/02/03 exact)'
$ringsOut = & $exe rings $fixture 2>&1 | Out-String
$ringsOut -split "`r?`n" | Where-Object { $_ -ne '' } | Select-Object -Last 5 | ForEach-Object { Write-Host "  $_" }
$ringsOk = ($LASTEXITCODE -eq 0) -and ($ringsOut -match 'RING-01') -and ($ringsOut -match 'RING-02') -and ($ringsOut -match 'RING-03') -and ($ringsOut -match 'detected=3 expected=3') -and ($ringsOut -match 'verdict=PASS')
Add-Result 'rings P1-003' "exit=$LASTEXITCODE" $ringsOk

Write-Host '[6/6] db test suites (sqa_ingest + sqa_graph)'
cargo test -p tessera_core --features db --test sqa_ingest --test sqa_graph 2>&1 | Select-String 'test result' | ForEach-Object { Write-Host "  $_" }
Add-Result 'cargo test (db suites)' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host ''
$results | Format-Table -AutoSize | Out-Host
$failed = @($results | Where-Object { $_.Result -eq 'FAIL' }).Count
if ($failed -gt 0) {
    Write-Host ("SQA-DB: FAIL ({0} of {1} steps failed). P1-002/P1-003 NOT accepted." -f $failed, $results.Count)
    exit 1
}
Write-Host ("SQA-DB: PASS ({0} of {1}). P1-002 + P1-003 accepted." -f $results.Count, $results.Count)
exit 0
