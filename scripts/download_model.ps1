param([string]$Dest = 'D:\tessera\models\moondream2')

$ErrorActionPreference = 'Stop'

$base = 'https://huggingface.co/vikhyatk/moondream2/resolve/main'
$files = @(
    [pscustomobject]@{ Name = 'model.safetensors';      Sha256 = '70a7d94c0c8349eb58ed2d9e636ef2d0916960f321ecabeac6354b8ba3d7403f'; Size = 3854538968 },
    [pscustomobject]@{ Name = 'tokenizer.json';         Sha256 = '337da36be7a71a6e88aa9148967a7bc8736f4b47c7de8e19ba92b89e80734cfc'; Size = 2114924 },
    [pscustomobject]@{ Name = 'config.json';            Sha256 = 'c4d59ae1179c1792ad49b8aeb59092101cc948d7b1914ed549689aed2c1fa083'; Size = 277 },
    [pscustomobject]@{ Name = 'generation_config.json'; Sha256 = 'a5a8484e27670c431bf1c5c9f972c27bdb8a3873ede65e2118440115b4c8d770'; Size = 69 }
)

Write-Host 'TESSERA // MODEL PROVISIONING // vikhyatk/moondream2 (weights are not evidence; D:-only, SHA-256 gated)'
Write-Host ("Destination: {0}" -f $Dest)
New-Item -ItemType Directory -Force -Path $Dest | Out-Null

$failed = 0
foreach ($f in $files) {
    $target = Join-Path $Dest $f.Name
    if ((Test-Path $target) -and ((Get-FileHash $target -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $f.Sha256)) {
        Write-Host ("SKIP  {0} (already present, hash verified)" -f $f.Name)
        continue
    }
    Write-Host ("GET   {0} ({1:N0} bytes)" -f $f.Name, $f.Size)
    $tmp = "$target.download"
    & curl.exe --fail --location --retry 3 --output $tmp "$base/$($f.Name)"
    if ($LASTEXITCODE -ne 0) {
        Write-Host ("FAIL  {0}: download error (exit {1})" -f $f.Name, $LASTEXITCODE)
        Remove-Item $tmp -ErrorAction SilentlyContinue
        $failed++
        continue
    }
    $hash = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -cne $f.Sha256) {
        Write-Host ("FAIL  {0}: sha256 {1} != expected {2} - FORENSIC BREAK, download deleted" -f $f.Name, $hash, $f.Sha256)
        Remove-Item $tmp -ErrorAction SilentlyContinue
        $failed++
        continue
    }
    Move-Item $tmp $target -Force
    Write-Host ("OK    {0} sha256={1}..." -f $f.Name, $hash.Substring(0, 16))
}

if ($failed -gt 0) {
    Write-Host ("MODEL PROVISIONING: FAIL ({0} file(s) rejected)" -f $failed)
    exit 1
}
$total = (Get-ChildItem $Dest -File | Measure-Object Length -Sum).Sum
Write-Host ("MODEL READY at {0} | total {1:N0} bytes | all SHA-256 verified" -f $Dest, $total)
Write-Host 'Next: D:\tessera\target\release\tessera_core.exe anomaly --envelope-only'
exit 0
