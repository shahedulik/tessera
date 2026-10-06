$ErrorActionPreference = 'Stop'

$header = 'D:\NVIDIA\CUDA\v13.4\include\cccl\cuda\std\__cccl\preprocessor.h'

if (-not (Test-Path $header)) {
    Write-Warning 'CUDA CCCL header not found; patch skipped.'
    exit 0
}

$item = Get-Item $header
if ($item.IsReadOnly) {
    $item.IsReadOnly = $false
}

$lines = [System.IO.File]::ReadAllLines($header)
$marker = 'TESSERA-DEV21-SUPPRESSION'

if (($lines -join "`n") -notmatch [regex]::Escape($marker)) {
    $lines = @(
        "// $marker",
        '#define CCCL_IGNORE_MSVC_TRADITIONAL_PREPROCESSOR_WARNING 1'
    ) + $lines
}

$lines = $lines | ForEach-Object {
    if ($_ -match '^\s*#\s*error' -and $_ -match 'traditional preprocessor') {
        "// TESSERA-DEV21-COMMENTED: $_"
    } else {
        $_
    }
}

[System.IO.File]::WriteAllLines($header, $lines)

$residual = @(
    [System.IO.File]::ReadAllLines($header) |
    Where-Object { $_ -match '^\s*#\s*error' -and $_ -match 'traditional preprocessor' }
).Count

if ($residual -ne 0) {
    throw "CUDA patch failed: residual active #error count=$residual"
}

Write-Host 'CUDA header patch verified (idempotent).' -ForegroundColor Green