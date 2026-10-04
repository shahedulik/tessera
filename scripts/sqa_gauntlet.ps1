$ErrorActionPreference = 'Continue'

$root = 'D:\tessera'
$fixture = Join-Path $root 'evidence\synthetic\synthetic_tenders.csv'
$sealedFixture = '2fe27f0f06c362361a407c34df53b3fc3abe9534effcf3e7a78c01421e61a8a4'
$manifest = Join-Path $root 'PHASE1_DNA_MANIFEST.sha256'
$exe = Join-Path $root 'target\debug\tessera_core.exe'

$results = @()

function Add-Result {
    param([string]$Step, [string]$Detail, [bool]$Pass)
    $script:results += [pscustomobject]@{
        Step   = $Step
        Result = $(if ($Pass) { 'PASS' } else { 'FAIL' })
        Detail = $Detail
    }
}

Set-Location $root
Write-Host 'TESSERA // SQA GAUNTLET // v1.0'
Write-Host ("Host: {0}  |  {1}" -f $env:COMPUTERNAME, (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ''

Write-Host '[1/5] clippy --all-targets --all-features -- -D warnings'
cargo clippy --all-targets --all-features -- -D warnings 2>&1 | Select-Object -Last 3 | ForEach-Object { Write-Host "  $_" }
Add-Result 'clippy -D warnings' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[2/5] cargo test --workspace --all-features'
cargo test --workspace --all-features 2>&1 | Select-Object -Last 8 | ForEach-Object { Write-Host "  $_" }
Add-Result 'cargo test (unit + integration)' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[3/5] proptest fuzz (10k cases, zero panics)'
cargo test --all-features --test sqa_benford fuzz 2>&1 | Select-Object -Last 4 | ForEach-Object { Write-Host "  $_" }
Add-Result 'proptest fuzz' "exit=$LASTEXITCODE" ($LASTEXITCODE -eq 0)

Write-Host '[4/5] SHA-256 gates (fixture seal + pack manifest)'
$fixtureOk = $false
if (Test-Path $fixture) {
    $fixtureHash = (Get-FileHash $fixture -Algorithm SHA256).Hash.ToLowerInvariant()
    $fixtureOk = ($fixtureHash -ceq $sealedFixture)
    Write-Host "  fixture: $fixtureHash"
}
Add-Result 'fixture seal' $sealedFixture.Substring(0, 16) $fixtureOk
$manifestFail = 0
if (Test-Path $manifest) {
    Get-Content $manifest | ForEach-Object {
        $parts = $_ -split '  '
        if ($parts.Count -ge 2) {
            $expected = $parts[0].ToLowerInvariant()
            $file = Join-Path $root $parts[1]
            if (-not (Test-Path $file)) {
                $manifestFail++
                Write-Host "  MISSING: $($parts[1])"
            } elseif ((Get-FileHash $file -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) {
                $manifestFail++
                Write-Host "  MISMATCH: $($parts[1])"
            }
        }
    }
} else {
    $manifestFail = -1
}
Add-Result 'pack manifest' "failures=$manifestFail" ($manifestFail -eq 0)

Write-Host '[5/5] benford runtime acceptance (chi2 / p / FLAG)'
$benfordOk = $false
if (Test-Path $exe) {
    $out = & $exe benford 2>&1 | Out-String
    $out -split "`r?`n" | Select-Object -Last 5 | ForEach-Object { Write-Host "  $_" }
    $benfordOk = ($LASTEXITCODE -eq 0) -and ($out -match 'chi2 = 199\.7') -and ($out -match 'FLAG \| BENFORD verdict=DETECTABLE')
    Add-Result 'benford acceptance' "exit=$LASTEXITCODE" $benfordOk
} else {
    Add-Result 'benford acceptance' 'exe missing - steps 1-2 build it' $false
}

Write-Host ''
$results | Format-Table -AutoSize | Out-Host
$failed = @($results | Where-Object { $_.Result -eq 'FAIL' }).Count
if ($failed -gt 0) {
    Write-Host ("GAUNTLET: FAIL ({0} of {1} steps failed). Module NOT done." -f $failed, $results.Count)
    exit 1
}
Write-Host ("GAUNTLET: PASS ({0} of {1}). SQA mandate satisfied." -f $results.Count, $results.Count)
exit 0
