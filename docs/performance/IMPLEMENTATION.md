# Rust performance implementation tracker

Authoritative objective: complete the Rust performance route in the installed desktop's `repair-checks/Rust性能优化路线.md`. Keep official kernel packages replaceable, original session data readable without acceleration, and user functionality intact. Do not enable the old Rust prototypes.

Branch: `perf/rust-data-path`. Existing proxy-DNS changes in the working tree predate this task and must be preserved.

| Unit | Owned files | Acceptance | State |
|---|---|---|---|
| Baseline and cache policy | scripts/performance/*, docs/performance/results/* | Official backend; small/medium/large fixtures; cache 1/2/5 comparison; process and heap measurements; cold/hot distinguished | verified; retain cache size 5 (smaller policies regress hot switching) |
| Rust helper | native-helper/* | Bounded streaming I/O, independent index, incremental transactions, cancellation/failure tests, no writes to original logs | implemented and tested: quotas, LRU, large-message chunks, revision-safe search |
| Desktop bridge | desktop/native/* | Lazy single child, bounded IPC, lifecycle shutdown, unknown-version fallback, same result contract | implemented; full isolated CLI and history WebSocket validated |
| Plugin integration | selected external plugin sources and desktop adapter | Incremental statistics/search/file processing actually used, no feature loss; integration evidence | Advisor parity 136 tests and benchmarks passed; native search tool exercised; side-panel candidate rejected by performance gate |
| Session read path | versioned desktop adapter, official boundary tests | Less intermediate allocation; packed/provenance/repair/unknown-version parity; no private cache deletion | Node-API prefetch implemented and differential tests passed; true page-source API unavailable in 0.1.2, official behavior retained |
| Packaging and deployment | src-tauri resource/build entries, installation manifest | Source and installed artifacts match, helper lifecycle owned, rollback works | completed: 0.2.5 installed, 17 file and backup hashes verified; installed mirror/CLI/WebSocket passed; NSIS bundle exit 0; final-artifact-audit.json records the expected three-byte build-tree/NSIS bundle-marker difference |
| Whole-route verification | docs/performance/results/* | Target p95 or peak total private memory improves >=20%; core scenarios <=5% time regression; correctness/cancel/fallback; actual UI interaction latency measured where available | completed for selected data paths: final history/index/balanced-policy gates pass; completion-audit.json records evidence and unmeasured whole-UI/model boundaries |

True disk-backed history pagination depends on an official range-source contract. Investigate public seams before determining whether this is currently implementable; do not count a read-only preview as a resumed Session. Keep any unsatisfied requirement explicit.

No unit is complete solely because its code compiles. Update evidence paths and state as work lands. Measurements use separate fixture roots and serial runs; don't restart the real desktop while the user is operating it.
