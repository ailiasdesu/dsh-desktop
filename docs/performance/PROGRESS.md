# Performance route implementation progress

2026-09-06. Goal remains active. The 0.2.5 runtime files and selected Advisor patch are installed. The pre-existing user window was deliberately not closed; normal exit/reopen activates the new image.

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

## Current release checkpoint

- Native history reads now use an optional Node-API addon with bounded 4 MiB prefetch. Public loadStored override delegates event decoding, replay validation, durable writes and repairs to official packages. No vendor kernel source was patched.
- Differential tests cover packed rows/provenance, Unicode/frame boundaries, corrupt/truncated/checksum input, all skippable magics and unsupported descriptor bits, version mismatch, file replacement/append during reads, and cancellation. Final Node suite: 24 tests pass. Native helper Rust suite contains protocol/cache/chunks/stream tests; shell suite: 23 tests pass.
- Full isolated DSH CLI verified custom persistence root/cache/packing config preservation, native search through the real tool pipeline, actual history WebSocket snapshot, stock fallback, and clean exit. The fixture has a real workspace header; original event prefix is preserved. Normal UI lifecycle metadata is compared with the stock mode, not falsely classified as decoder mutation.
- Last successful large-history benchmark (`results/history-addon-prefetch/summary.json`): 128 MiB synthetic history, five independent processes per mode; p95 908.45 -> 648.62 ms, median total private peak 522.95 -> 352.70 MiB. Those values are the named benchmark artifact, not a blanket real-user speedup promise. Later correctness fixes require final release smoke/verification, not silent relabeling of old measurements.
- Advisor incremental unit: 136 parity tests, no full snapshots on normal event streams; deployment is limited to the three files in its manifest. Its isolated 100000-turn benchmark improves observer p95 52.41 -> 0.14 ms and peak RSS 461.56 -> 165.42 MiB; this is not whole-app RSS.
- Side-panel candidate is NOT selected for installation: it improved repeated preview but regressed six-file rotation and retained more memory. Its source/measurements remain in the excluded experimental folder for future work. Existing side-panel UI/runtime remain unchanged.
- Raw hash/slice primitives are built and measured, but the current official Fs provider lacks a binary-stream/hash capability seam. Do not bypass provider semantics or perform a second full read merely to claim Rust integration. File primitive benchmark remains explicitly separate from attachment/UI performance.
- Actual cold history profile shows remaining synchronous official title-normalization/projection work after decoding. Replacing those business reducers would fight the update boundary; they remain official. True disk pagination is conditional on a future public range-source API and is not falsely represented by a preview.
- Confirmed review fixes: scoped expected_revision is checked in the same SQLite snapshot even for zero hits; install and rollback journals recover the renamed-but-not-replaced executable state; bytecode is excluded. Deployment tests: 9 pass, including a real running Windows image replacement without stopping it.
- Source packaging baseline synchronized to the installed 0.1.2-rc.1 bundle (24998 files, no links); old source bundle retained. Shell version is 0.2.5. Release build runs in a hidden process with durable logs/status under target/release-build-*.

## Final installation checkpoint

17 installed files verified against the backed-up manifest; installed mirror smoke returned MIRROR_SMOKE_OK; installed-module full CLI/history WebSocket/tool and stock fallback passed. Original 21 compatibility tests and 45-file check passed. The source-config checksum refresh and balanced-read-policy refresh retain prior bytes and provenance in the same deployment backup.

Final history benchmark: p95 653.93 -> 470.56 ms, total private peak 522.72 -> 356.43 MiB. Default native threshold is now 32 MiB compressed: the small/mid history policy benchmark stays on stock reads (p95 110.34 vs 110.32 ms), avoiding the 4 MiB threshold's marginal-regression case. Final text-index total private peak is 410.02 -> 87.98 MiB.

All 13 selected review roles completed. Confirmed findings fixed and verified; advisory limitations documented. The optional side-panel candidate remains rejected, not installed. Remaining operational work: finish installer-only repack, record final package hash and completion audit, commit delivery records. No further feature code is pending.
