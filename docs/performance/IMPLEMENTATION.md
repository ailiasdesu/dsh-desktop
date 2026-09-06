# Rust performance implementation tracker

Authoritative objective: complete the Rust performance route in the installed desktop's `repair-checks/Rust性能优化路线.md`. Keep official kernel packages replaceable, original session data readable without acceleration, and user functionality intact. Do not enable the old Rust prototypes.

Branch: `perf/rust-data-path`. Existing proxy-DNS changes in the working tree predate this task and must be preserved.

| Unit | Owned files | Acceptance | State |
|---|---|---|---|
| Baseline and cache policy | scripts/performance/*, docs/performance/results/* | Official backend; small/medium/large fixtures; cache 1/2/5 comparison; process and heap measurements; cold/hot distinguished | in progress |
| Rust helper | native-helper/* | Bounded streaming I/O, independent index, incremental transactions, cancellation/failure tests, no writes to original logs | implemented base; large-document/cache eviction work remains |
| Desktop bridge | desktop/native/* | Lazy single child, bounded IPC, lifecycle shutdown, unknown-version fallback, same result contract | implemented and tested library; production plugin integration remains |
| Plugin integration | selected external plugin sources and desktop adapter | Incremental statistics/search/file processing actually used, no feature loss; integration evidence | pending |
| Session read path | versioned desktop adapter, official boundary tests | Less intermediate allocation; packed/provenance/repair/unknown-version parity; no private cache deletion | pending |
| Packaging and deployment | src-tauri resource/build entries, installation manifest | Source and installed artifacts match, helper lifecycle owned, rollback works | pending |
| Whole-route verification | docs/performance/results/* | Target p95 or peak total private memory improves >=20%; core scenarios <=5% time regression; correctness/cancel/fallback; actual UI interaction latency measured where available | pending |

True disk-backed history pagination depends on an official range-source contract. Investigate public seams before determining whether this is currently implementable; do not count a read-only preview as a resumed Session. Keep any unsatisfied requirement explicit.

No unit is complete solely because its code compiles. Update evidence paths and state as work lands. Measurements use separate fixture roots and serial runs; don't restart the real desktop while the user is operating it.
