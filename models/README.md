# D:\tessera\models — LOCAL FORGE artifact directory

AMD cloud credits UNAVAILABLE — Phase 2 runs entirely on the local RTX 5060 (8GB) within the
envelope 4608MB weights / 2560MB KV+vectors / 1024MB overhead.

Contents policy:
- Quantized vision weights (INT4/AWQ-class, candle safetensors) produced by prompt P2-001.
- Every artifact ships with a `<name>.sha256` manifest line; mismatch on load = forensic break = reject.
- Zero evidence ever leaves local silicon; only public model weights are downloaded (HF_HOME=D:\tessera-cache\hf).
- This directory is git-ignored for large binaries; manifests ARE committed.
