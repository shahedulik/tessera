# TESSERA — Cursor Composer Prompt Register v2 (identity rebirth edition)
# Cursor Pro expiry 2026-09-30: every prompt documented here; post-expiry engine = Qwen Coder + .qwenrules.
# AMD cloud credits UNAVAILABLE -> Phase 2 is the LOCAL FORGE path (RTX 5060, D:\tessera\models).
# SQA MANDATE: every prompt's output must pass scripts/sqa_gauntlet.ps1 before merge (clippy -D warnings, tests, 10k-case proptest zero-panic fuzz, SHA-256 fixture gates).

## ENTRY FORMAT (mandatory)
- ID / Date / Tool / Target files / Prompt (verbatim) / Expected outcome / SQA gate

---

## P1-001 — Identity rebirth + spine (EXECUTED by Lead Architect forge, 2026-09-16)
- Targets: full workspace (see PHASE1_DNA_MANIFEST.sha256).
- Outcome: TESSERA v13.1 skeleton; benford.rs spine with acceptance chi2=199.733±0.01, p=7.274221e-39, FLAG lines; sqa_benford.rs + sqa_pools.rs; gauntlet; DEV-10..15 pins carried (candle 0.11/cudarc 0.19.x, arrow 58, duckdb bundled 1.10505.0, kuzu 0.11.3 prebuilt default-features=false, cxx/cxx-build =1.0.138, pdfium ABI-6996 vendored).
- Gate: sqa_gauntlet.ps1 -> 6/6 PASS.

## P1-002 — DuckDB ingestion of sealed fixture (EXECUTED + ACCEPTED in Lead Architect forge)
- Outcome: crates/tessera_core/src/ingest.rs + tests/sqa_ingest.rs. Accepted: custody PASS, rows=5000, duckdb v1.5.5 count(*)=5000, chi2=199.733 p=7.274221e-39 reproduced in-pipeline, 5/5 tests incl. 10k-case malformed-rows fuzz, clippy clean.
- Prompt: "Per .cursorrules: in crates/tessera_core behind feature `db`, add ingestion.rs that reads D:\tessera\evidence\synthetic\synthetic_tenders.csv via benford::parse_csv_bytes (Arrow RecordBatch, zero re-parse), verifies the SHA-256 seal 2fe27f0f... at ingestion, then bulk-inserts into in-memory DuckDB table `tenders` using the duckdb Appender API. CLI: `tessera_core.exe ingest`. Log rows=5000 and the seal hash. Ship with SQA (a)-(d): proptest fuzz over malformed batch inputs (10k cases, zero panics), fixture hash gate test, clippy clean."
- Expected: `INGEST | rows=5000 | sha256=2fe27f0f...` then DuckDB `SELECT count(*) FROM tenders` = 5000.
- Gate: gauntlet PASS + acceptance numbers above.

## P1-003 — Kuzu bipartite graph + Circular Flow template (EXECUTED + ACCEPTED in Lead Architect forge)
- Outcome: crates/tessera_core/src/graph.rs + tests/sqa_graph.rs. Accepted: schema 4 node + 4 rel tables; loaded companies=4960 persons=4947 tenders=5000 OWNED_BY=4951 BID_ON=5000 TRANSFERRED_TO=9 SHARES_ADDRESS=4; rings detected=3 expected=3 (RING-01/02/03 exact, tx sets + spans verified, negative test: hop removal -> 2 rings); 5/5 tests incl. 10k escape-roundtrip fuzz; clippy clean.
- Prompt: "Per .cursorrules: add graph.rs behind feature `db`. Kuzu schema: nodes Person(name PK), Company(name PK), Tender(id PK, amount DOUBLE, date DATE, district STRING); rels OWNED_BY(Company->Person), BID_ON(Company->Tender), TRANSFERRED_TO(Company->Company, amount DOUBLE, date DATE). Load from DuckDB `tenders` via Arrow C Data Interface (zero-copy). Loader rule from evidence/synthetic/injection_manifest.json: owner_name matching a known company_name emits TRANSFERRED_TO; else OWNED_BY + BID_ON. Implement pre-compiled Circular Flow template: MATCH (a:Company)-[:TRANSFERRED_TO]->(b:Company)-[:TRANSFERRED_TO]->(c:Company)-[:TRANSFERRED_TO]->(a) WHERE all three hops within 7 days. Ship with SQA (a)-(d)."
- Expected: exactly 3 cycles (RING-01/02/03 ground truth); ring amounts decay 0.4-1.5%/hop.
- Gate: gauntlet PASS + cycle count == 3.

## P1-004 — Alias resolution (identity gate)
- Prompt: "Per .cursorrules: add identity.rs — deterministic Jaro-Winkler + Levenshtein gate over company_name pairs sharing owner_name; collapse confirmed aliases into Super-Node keys before any graph query (Manual s5.1). Ground truth: 4 alias pairs in injection_manifest.json (incl. Tongi Builders Ltd / Tongi Bldrs Limited). No new heavy deps; pure Rust in Pool B style. Ship with SQA (a)-(d) incl. fuzz over adversarial unicode/empty strings (10k cases)."
- Expected: 4/4 pairs collapsed; zero false merges of the 4,823 baseline companies.
- Gate: gauntlet PASS + recall 4/4, precision 1.0 on fixture.

## P1-005 — Vision rasterization gate (pdfium proof)
- Prompt: "Per .cursorrules: add a `raster <pdf>` CLI subcommand to tessera_core that constructs tessera_vision::VisionEngine (pdfium.dll staged beside exe), rasterizes page 1 at 1024px, computes the layout SHA-256, exercises the ghost cache (insert/lookup/bounds), and prints tensor shape + device. Ship with SQA (a)-(d) using a sealed 1-page fixture PDF under evidence/ (hash-gated)."
- Expected: `[VDU] Rasterized ... Shape: [1024, H, 3]`, cache round-trip PASS.
- Gate: gauntlet PASS on a machine with the staged DLL.

## P2-001 — LOCAL FORGE: INT4/AWQ-class quantized VLM (replaces AMD cloud path)
- Prompt: "Per .cursorrules LOCAL FORGE ONLY: add hf-hub to [workspace.dependencies]; download Qwen2-VL-2B-Instruct (weights are not evidence; HF_HOME=D:\tessera-cache\hf) and implement crates/tessera_vision quantize pipeline with candle: per-group INT4 symmetric quantization of linear layers (AWQ-class activation-aware scaling using a SYNTHETIC calibration set generated locally — never real evidence), KV-cache budget check against 2560MB, weights budget against 4608MB. Export to D:\tessera\models\qwen2vl2b_int4.safetensors + sha256 manifest + VRAM probe log from the RTX 5060 (cudarc allocation high-water mark). Ship with SQA (a)-(d): quantize/dequantize round-trip error bounds as property tests (10k random tensors), envelope invariants, zero panics."
- Expected: artifact + manifest in D:\tessera\models; probe <= 4608MB weights.
- Gate: gauntlet PASS + envelope probe within budget.

## P4-001 — Quarto forensic dossier
- Prompt: "Per .cursorrules: generate Quarto dossier pipeline consuming benford/graph/identity outputs: clickable bbox deep-links, JSON-LD + D3.js graph export, SHA-256 provenance chain from the sealed fixture, signed PDF output to D:\tessera\dossiers."
- Gate: dossier renders; every flag traces to fixture row ids; signature verifies.
