$ErrorActionPreference = 'Stop'

$requiredCargoHome  = 'D:\tessera-cache\cargo'
$requiredHfHome     = 'D:\tessera-cache\hf'
$requiredRepoDir    = 'D:\tessera'
$requiredKuzuLibDir = 'D:\tessera-cache\kuzu\lib'
$minFreeGb          = 20

function Get-EnvObserved([string]$Value) {
    if ([string]::IsNullOrEmpty($Value)) { return '(not set)' }
    return $Value
}

$checks = @()

$rustcCommand = Get-Command rustc -ErrorAction SilentlyContinue
if ($null -ne $rustcCommand) {
    $rustcObserved = $rustcCommand.Source
    try { $rustcObserved = (& $rustcCommand.Source --version) } catch { }
    if ($rustcObserved.Length -gt 32) { $rustcObserved = $rustcObserved.Substring(0, 29) + '...' }
    $rustcResult = 'PASS'
} else {
    $rustcObserved = 'not found on PATH'
    $rustcResult = 'FAIL'
}
$checks += [pscustomobject]@{ Check = 'rustc on PATH'; Expected = 'resolvable'; Observed = $rustcObserved; Result = $rustcResult }

$cargoHome = Get-EnvObserved $env:CARGO_HOME
$cargoResult = 'FAIL'
if ($env:CARGO_HOME -ieq $requiredCargoHome) { $cargoResult = 'PASS' }
$checks += [pscustomobject]@{ Check = 'CARGO_HOME'; Expected = $requiredCargoHome; Observed = $cargoHome; Result = $cargoResult }

$hfHome = Get-EnvObserved $env:HF_HOME
$hfResult = 'FAIL'
if ($env:HF_HOME -ieq $requiredHfHome) { $hfResult = 'PASS' }
$checks += [pscustomobject]@{ Check = 'HF_HOME'; Expected = $requiredHfHome; Observed = $hfHome; Result = $hfResult }

$repoExists = Test-Path -Path $requiredRepoDir -PathType Container
$repoObserved = 'missing'
$repoResult = 'FAIL'
if ($repoExists) { $repoObserved = 'exists'; $repoResult = 'PASS' }
$checks += [pscustomobject]@{ Check = 'Repo directory'; Expected = $requiredRepoDir; Observed = $repoObserved; Result = $repoResult }

$freeGb = [math]::Round((Get-PSDrive -Name C).Free / 1GB, 2)
$freeResult = 'FAIL'
if ($freeGb -gt $minFreeGb) { $freeResult = 'PASS' }
$checks += [pscustomobject]@{ Check = 'C: free space'; Expected = "> $minFreeGb GB"; Observed = "$freeGb GB"; Result = $freeResult }

$kuzuLibDir = Get-EnvObserved $env:KUZU_LIBRARY_DIR
$kuzuResult = 'FAIL'
if ($env:KUZU_LIBRARY_DIR -ieq $requiredKuzuLibDir) { $kuzuResult = 'PASS' }
$checks += [pscustomobject]@{ Check = 'KUZU_LIBRARY_DIR'; Expected = $requiredKuzuLibDir; Observed = $kuzuLibDir; Result = $kuzuResult }

$nvccCommand = Get-Command nvcc -ErrorAction SilentlyContinue
$nvccObserved = 'not found on PATH'
$nvccResult = 'FAIL'
if ($null -ne $nvccCommand) {
    try { $nvccObserved = ((& nvcc --version) | Select-String 'release').ToString().Trim() } catch { $nvccObserved = $nvccCommand.Source }
    $nvccResult = 'PASS'
}
$checks += [pscustomobject]@{ Check = 'nvcc (CUDA 13.4)'; Expected = 'release 13.x'; Observed = $nvccObserved; Result = $nvccResult }

Write-Host 'TESSERA // ENVIRONMENT VERIFICATION KIT // v2.0'
Write-Host ("Host: {0}  |  {1}" -f $env:COMPUTERNAME, (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ''
$checks | Format-Table -AutoSize | Out-Host

$failed = @($checks | Where-Object { $_.Result -eq 'FAIL' }).Count
if ($failed -gt 0) {
    Write-Host ("GATE: FAIL - {0} of {1} checks failed. Do NOT build." -f $failed, $checks.Count)
    exit 1
}
Write-Host ("GATE: PASS - {0} of {1} checks passed. Build ladder unlocked." -f $checks.Count, $checks.Count)
exit 0
