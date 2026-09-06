# Performance route delivery status

2026-09-06. Selected compatible optimizations are implemented, installed and verified in desktop 0.2.5 with official kernel 0.1.2-rc.1. See RELEASE.md and completion-audit.json for the final scope and evidence. No user process was forcibly stopped; normal next launch activates the installed runtime.

## Delivered

- Rust helper: bounded protocol, lazy single process, owned SQLite index, incremental transactions, revision-safe search, large-message chunks, capacity/LRU, graceful shutdown, file primitives.
- Node-API reader: asynchronous bounded 4 MiB prefetch for compressed histories at least 32 MiB. Official decoding, replay, writes, repair and lifecycle remain authoritative. Unknown versions/native failures use the official path.
- Advisor: incremental JavaScript event processing, no full snapshot on normal appends, no unused all-history fingerprint. Only three manifest-owned files installed; original UI retained.
- Desktop resources and conditional activation packaged; transactional deployment and rollback preserve backups and running Windows images.

## Final results and checks

- history-final: 128 MiB synthetic history p95 653.93 to 470.56 ms; median sampled total private peak 522.72 to 356.43 MiB, five independent processes each.
- history-balanced-final: smaller histories stay on stock reads; p95 110.34 vs 110.32 ms. The earlier 4 MiB threshold was replaced after a marginal regression.
- index-final: already-built literal text index p95 101.08 to 94.27 ms; total private peak 410.02 to 87.98 MiB, including helper. Initial index construction remains separate.
- Advisor observer-only 100000-turn fixture: per-new-turn p95 52.41 to 0.14 ms. Not whole-app RSS or complete model-turn latency.
- 24 Node tests, 31 helper Rust tests, four final scoped-search regressions, nine deployment tests and 136 Advisor parity tests passed. Shell tests passed in the preceding verification checkpoint.
- Installed mirror smoke and full isolated CLI/history WebSocket/current-session search/config preservation/stock fallback passed.
- Final installed compatibility check: 21 tests and 45 patched-file hash/syntax checks passed.
- All 13 selected review roles completed; confirmed findings fixed. See review/caller-resolutions.json.
- Final NSIS build exited 0. Final artifact audit verifies 17 installed files and original backups, package SHA-256, source payload parity and the expected three-byte Tauri EXE bundle-marker difference.

## Evaluated limits

- Prepared cache reduction and side-panel candidate failed selected performance gates; neither was installed. The rejected side-panel experiment remains untracked and contains a node_modules junction; do not recursively delete it.
- Hash/slice primitives are built and measured; no official binary-stream/hash provider seam exists for transparent attachment integration. Attachment/UI gains are not claimed.
- True disk pagination awaits a public upstream range-source contract. Complete event residency and official synchronous title/projection work remain.
- No uniform whole-UI, live-model or arbitrary-workload speedup is claimed. Remote model inference is unchanged.
- New official versions disable unverified acceleration. Third-party Advisor upgrades can overwrite its local patch and require fresh compatibility validation.

No further selected runtime implementation or packaging work is pending. Final delivery records are committed separately from the implementation. Original roadmap remains unchanged as the prior assessment.
