# TESSERA — CURSOR HANDOFF PACK (v13.1, 2026-09-16)

Prepared by: Lead Architect. For: Commander (non-STEM) operating Cursor Pro on the Fortress.
Supersedes: all AURA-AUDIT-era instructions. Identity is TESSERA everywhere.

## 0. ENVIRONMENT FACTS (verified state)
- Root: `D:\tessera` (git repo, origin `https://github.com/shahedulik/tessera`, origin/main = v12.0 core: 677 objects, tree contains `src/main.rs`, root `Cargo.toml`, `flake.nix`, `flake.lock`, `flake.nix.save`, `qodana.yaml`, `.idea/aura-audit.iml`, `.gitignore`, `Cargo.lock`).
- Caches: `D:\tessera-cache\{cargo,rustup,hf,vcpkg-root,kuzu}`. CUDA 13.4 at `D:\NVIDIA\CUDA\v13.4`. rustc 1.98.1, git 2.55.0, vcpkg 2026-07-27.
- AMD cloud credits: UNAVAILABLE. Phase 2 = LOCAL FORGE (RTX 5060, envelope 4608/2560/1024 MB, artifacts to `D:\tessera\models`).

## 1. PACK PLACEMENT
Download every file from the workspace `tessera/` tree to the identical relative path under `D:\tessera\` (full list = `PHASE1_DNA_MANIFEST.sha256`, which cannot list itself).
Then in PowerShell:

```powershell
cd D:\tessera
$fail = 0
Get-Content PHASE1_DNA_MANIFEST.sha256 | ForEach-Object {
  $h, $f = $_ -split '  '
  if (-not (Test-Path $f)) { $fail++; Write-Host "MISSING  $f" }
  elseif ((Get-FileHash $f -Algorithm SHA256).Hash.ToLowerInvariant() -cne $h) { $fail++; Write-Host "MISMATCH $f" }
  else { Write-Host "PASS     $f" }
}
Write-Host "manifest failures: $fail"
```

GATE 1: `manifest failures: 0`. Any failure = forensic break = re-download that file.

## 2. GIT INTEGRATION PLAN (v12.0 history preserved; rebirth committed on top)
Exact commands, in order:

```powershell
cd D:\tessera
git fetch origin
git branch --list main
```

- If `main` is NOT listed: `git switch -c main origin/main`
- If `main` IS listed: `git switch main` then `git merge --ff-only origin/main`

Then delete the v12.0 flat layout and the Nix/IDE contraband (Windows-native law; history stays in git forever):

```powershell
git rm -r -q src .idea flake.nix flake.lock flake.nix.save qodana.yaml
```

(Pack files from §1 overwrite the old root `Cargo.toml`, `Cargo.lock`, `.gitignore` automatically.)

```powershell
git add -A
git commit -m "TESSERA v13.x: identity rebirth + DEV pins"
git log --oneline -3
git push -u origin main
```

GATE 2: `git log --oneline -3` shows the rebirth commit on top of v12.0 history; push reports `main -> main`.

## 3. KUZU PREBUILT PROVISIONING (skip if `D:\tessera-cache\kuzu\lib\kuzu_shared.dll` already exists)

```powershell
New-Item -ItemType Directory -Force -Path D:\tessera-cache\kuzu\lib, D:\tessera-cache\kuzu\include | Out-Null
Invoke-WebRequest -Uri 'https://github.com/kuzudb/kuzu/releases/download/v0.11.3/libkuzu-windows-x86_64.zip' -OutFile 'D:\tessera-cache\kuzu\libkuzu-windows-x86_64.zip'
(Get-FileHash 'D:\tessera-cache\kuzu\libkuzu-windows-x86_64.zip' -Algorithm SHA256).Hash
```

GATE 3: hash equals `8C2297955F98CF23B45C3F69935E4EEBC6672DAC2E2FA00973F6EAAE00D20259`. Then:

```powershell
Expand-Archive -Path 'D:\tessera-cache\kuzu\libkuzu-windows-x86_64.zip' -DestinationPath 'D:\tessera-cache\kuzu\stage' -Force
Move-Item 'D:\tessera-cache\kuzu\stage\kuzu.h','D:\tessera-cache\kuzu\stage\kuzu.hpp' 'D:\tessera-cache\kuzu\include\' -Force
Move-Item 'D:\tessera-cache\kuzu\stage\kuzu_shared.lib','D:\tessera-cache\kuzu\stage\kuzu_shared.dll' 'D:\tessera-cache\kuzu\lib\' -Force
[Environment]::SetEnvironmentVariable('KUZU_LIBRARY_DIR','D:\tessera-cache\kuzu\lib','User')
[Environment]::SetEnvironmentVariable('KUZU_INCLUDE_DIR','D:\tessera-cache\kuzu\include','User')
[Environment]::SetEnvironmentVariable('KUZU_SHARED','1','User')
```

Close PowerShell, open a NEW window (env vars load per-session).

## 4. BUILD LADDER
Environment gate first:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\tessera\scripts\env_verify.ps1
```

GATE 4a: `GATE: PASS - 7 of 7` (rustc, CARGO_HOME, HF_HOME, repo dir, C: free >20GB, KUZU_LIBRARY_DIR, nvcc).

Skeleton build (both crates; ~5-15 min):

```powershell
cd D:\tessera
cargo build --release
```

GATE 4b: ends `Finished 'release' profile [optimized]`; warning line `pdfium.dll staged beside executable` appears; `Test-Path D:\tessera\target\release\tessera_core.exe` → `True`; `Test-Path D:\tessera\target\release\pdfium.dll` → `True`.

Orchestrator gate:

```powershell
D:\tessera\target\release\tessera_core.exe
```

GATE 4c: `Detected 32 logical cores`; TOKIO probe line; POOL_A cores 0-5, POOL_B cores 6-21, POOL_C cores 22-31; zero `Core unassigned`, zero `Failed to pin thread`; press ENTER → `High-Risk: 40.0%` on every POOL_B line → `--- TESSERA v13.1: MISSION COMPLETE ---`.

Spine gate (Benford, from any terminal):

```powershell
D:\tessera\target\release\tessera_core.exe benford
```

GATE 4d: `chain-of-custody: PASS (sealed fixture...)`; `chi2 = 199.733   df = 8   p-value = 7.274221e-39`; FLAG lines for digit 1 and digit 9; `FLAG | BENFORD verdict=DETECTABLE ...`.

Brain build (one-time DuckDB C++ amalgamation — console may look frozen 15-35 min; DO NOT cancel):

```powershell
cd D:\tessera
cargo build --release -p tessera_core --features db
D:\tessera\target\release\tessera_core.exe
```

GATE 4e: BRAIN lines: `DuckDB OLAP engine online | version v1.5.5` and `Kuzu graph brain online`; `Test-Path D:\tessera\target\release\kuzu_shared.dll` → `True`.

## 5. ORDERED COMPOSER PROMPTS (one per module — full text in cursor_prompts.md)
Execute strictly in order; after EACH module run the gauntlet (§6) and commit only on full PASS:
1. P1-002 DuckDB ingestion of the sealed fixture (feature `db`).
2. P1-003 Kuzu bipartite graph + Circular Flow template (must return exactly RING-01/02/03).
3. P1-004 Alias resolution Super-Node collapse (ground truth: 4 pairs).
4. P1-005 Vision rasterization gate (pdfium proof on a sealed fixture PDF).
5. P2-001 LOCAL FORGE quantization (Qwen2-VL-2B INT4/AWQ-class → D:\tessera\models, envelope-probed).
6. P4-001 Quarto dossier (bbox deep-links, JSON-LD + D3.js, signed PDF).

Cursor usage: open `D:\tessera` as the project (File → Open Folder). Composer reads `.cursorrules` automatically. Paste the prompt verbatim from cursor_prompts.md; disable any WSL extensions.

## 6. SQA GAUNTLET (supreme law runner)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\tessera\scripts\sqa_gauntlet.ps1
```

Runs in order: clippy (-D warnings, all-targets, all-features) → cargo test (unit+integration) → proptest fuzz (10k cases) → hash gates (fixture seal + full pack manifest) → benford runtime acceptance. GATE 6: final line `GAUNTLET: PASS (5 of 5). SQA mandate satisfied.` and exit code 0 (`echo $LASTEXITCODE`).

## 7. TROUBLESHOOTING QUICK TABLE
| Symptom | First move |
|---|---|
| `nvcc` not found at build | `$env:Path = "D:\NVIDIA\CUDA\v13.4\bin;$env:Path"`; persist via System env if it recurs |
| undefined `kuzu_rs$cxxbridge1$...` symbols | Cargo.lock cxx-build pin drifted — restore pack Cargo.lock (hash-gated) |
| `could not find native static library snowball` | kuzu default features re-enabled — must stay `default-features = false` |
| DuckDB compile looks frozen | Single-TU amalgamation; 15-35 min once; cached in D:\tessera\target |
| pdfium FATAL at runtime | pdfium.dll not beside exe — rerun build (build.rs stages it), check vendor hash |
| C: free space dropping | run scripts\env_verify.ps1, audit env vars, weekly clean; never retarget caches to C: |
| Benford chi2 off by >0.01 | fixture tampered or swapped — check seal 2fe27f0f...; do NOT recalibrate, re-download evidence |

## 8. CURSOR EXPIRY (2026-09-30)
All prompts live in cursor_prompts.md; `.qwenrules` is the byte-identical mirror of `.cursorrules`. Post-expiry: Qwen Coder (free) opens the same repo, reads `.qwenrules`, executes the same register. Nothing depends on Cursor-specific state.
