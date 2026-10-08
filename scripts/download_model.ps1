param([string]$Dest = 'D:\tessera\models\moondream2')

$ErrorActionPreference = 'Stop'

$revision = 'f6e9da68e8f1b78b8f3ee10905d56826db7a5802'
$base = "https://huggingface.co/vikhyatk/moondream1/resolve/$revision"
$files = @(
    [pscustomobject]@{ Name = 'model.safetensors';      Sha256 = '892e51df302d98a83974761c4f386caddbad2edd0e84f228d9935b4aed33ee25'; Size = 3715037856 },
    [pscustomobject]@{ Name = 'tokenizer.json';         Sha256 = '337da36be7a71a6e88aa9148967a7bc8736f4b47c7de8e19ba92b89e80734cfc'; Size = 2114924 },
    [pscustomobject]@{ Name = 'config.json';            Sha256 = '426ecbf99e0a057f55f162ba97479bdd6b7ed1759c4f966db38cd5fe3255500b'; Size = 323 },
    [pscustomobject]@{ Name = 'generation_config.json'; Sha256 = 'd8b3d56ccdc67e074c7923b07d778721d2d6212bacb344e97f9464fde1b7f29d'; Size = 69 }
)

Write-Host 'TESSERA // MODEL PROVISIONING // Moondream2 1.86B f16 - REVISION-PINNED (DEV-38)'
Write-Host ("Source: vikhyatk/moondream1 @ {0}" -f $revision)
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
