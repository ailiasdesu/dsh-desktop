# Performance route implementation progress

2026-09-06. Goal remains active. Nothing from this branch has been deployed into the running desktop yet.

## Implemented and exercised

- `native-helper`: separate Rust binary, bounded NDJSON protocol, streaming SHA256, bounded UTF-8 file slices, independent SQLite event-text cache, atomic replacement, incremental suffix update, revision checks against concurrent writers, request/data budgets.
- `desktop/native/client.mjs`: lazy single process, bounded outstanding requests and frames, cancellation/timeout, idle shutdown, graceful normal close, restart after failure. Normal close uses EOF so SQLite checkpoints instead of recovering a killed writer's WAL on every next launch.
- `desktop/native/session-text.mjs`: official event decoder/text extraction remain authoritative; cold-source revisions and live Session generation/cursor validate derived caches. Unknown kernel and native failures use the official observation path. This is literal per-event text search, NOT a replacement for official ranked FTS or session resume.
- Real Node-to-Rust-to-SQLite integration tests, official Session/persistence/query observation tests, and Rust file/transaction/protocol tests are present. Build with the installed Rust toolchain; rustfmt component was installed because it was missing.

## Measured evidence

`results/cache-hot-five/summary.json`: five fresh processes per cache policy, six highly compressible synthetic 16 MiB text histories, uncontrolled OS disk cache, diagnostic GC between phases. Cache 2 reduced median sampled peak private memory from 411.77 to 325.39 MiB, but rotating five histories regressed p95 from 2.27 to 75.82 ms. Cache 1 also regressed the two-history switch. **Do not deploy reduced cache size as a universal optimization.** Original default remains unchanged.

`results/index-graceful/summary.json`: five fresh processes per mode, 60 queries each, exact match/sequence assertions, native helper private memory included. An already-built independent index gave p50 49.03 ms vs official-observation literal scan 71.12 ms; p95 93.06 vs 114.91 ms. Median sampled peak total private memory 87.68 vs 409.09 MiB. These are synthetic search measurements, not actual session-open/UI/model speed claims. Initial index construction is a separate cost (~1.3–1.8 seconds per fixture in `results/index/initial-index-build.json`). Earlier `results/index` captures the diagnosed killed-WAL startup regression and must not be used as the final result.

The final helper subsequently gained lazy DB opening and cache/import limits. Repeat affected benchmarks before promotion; do not automatically transfer earlier numbers to a newer binary.

`results/files/summary.json`: final lazy-DB helper, five fresh processes per variant and three SHA256 checks each on a 256 MiB synthetic file. Streaming Node baseline was included: p95 385.84 ms and median peak total private memory 124.93 MiB; native 219.61 ms and 55.00 MiB. Whole-buffer Node was 381.80 ms and 566.13 MiB. All hashes matched. OS cache was uncontrolled and these are file-tool primitives, not attachment UI measurements. The native file path has a Windows opened-handle containment check; using it as a general DSH tool still requires preserving the official filesystem/permission boundary rather than bypassing it.

Latest verification: 10 Rust tests and 7 Node/official-service integration tests passed. Current helper is approximately 2 MiB. Commit `8ba6914` records the first implementation and search/cache evidence; file benchmarks are a subsequent checkpoint. No production configuration, original session file, or running desktop has been changed by this route implementation.

## Still required

1. Integrate native functions into real desktop/plugin user and agent entry points, preserving existing actions; presently these are executable/tested modules, not installed features.
2. Complete cache quota/eviction and large-document handling: bounded refusal currently falls back correctly but cannot yet accelerate every very large session. Do not truncate user content to get a passing performance result.
3. Add small/medium/large file and text-processing comparisons (including streaming Node baseline, not only whole-buffer Node), targeted plugin incremental-statistics work, and actual active-turn timing.
4. Implement/evaluate native streaming history-read assistance at public/versioned seams, preserving official repair/packed/provenance behavior. Investigate true page-source support; current official history path still materializes full events.
5. Validate production packaging, compatibility/version fallback, per-plugin regression tests, real background runtime integration, UI latency where accessible, and upgrade/rollback behavior.
6. Review code and re-measure final artifacts before deployment. No claim that all route requirements are finished.
