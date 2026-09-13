# Workspace UI baseline — Step 0

Engineering evidence only: no layout, viewer, Git-panel, or agent-retrieval
behavior changes. Syntax highlighting is not semantic analysis or retrieval proof.

## Identity and reproduction

- Baseline: `a592c38c16cdd7623dacc9168926ebccedfb67d3`, branch
  `hoplite/teos-45b5c307`, initially clean worktree; measured 2026-09-13.
- Linux x86-64 / Modal: 1 CPU, 2 GiB reservation; conditional ceiling 4 CPU,
  16 GiB. Shared-host timing is diagnostic, not a stable CI threshold.
- Rust 1.95.0; just 1.58.0; cargo-nextest 0.9.144. The sandbox initially had no
  Rust or built Codex binary. Installed native prerequisites: pkg-config,
  OpenSSL/ALSA/libclang headers; tmux for real-terminal checks.
- Built `codex-tui 0.0.0` and `codex-file-search` with the existing unoptimized
  dev profile. TUI binary SHA-256:
  `2869fdd6321bd70ed136a49aef921451ab05fe81022547703b707939ae049a2c`.
- Initial parallel compilation and a subsequent TUI test compilation terminated
  with SIGTERM, without Rust source diagnostics. The successful test build used
  one build worker, TUI-only `debug=0`, and 256 codegen units. This is not an
  optimized benchmark. **Nextest `-j` controls test concurrency, not compilation.**
- `just bazel-lock-update` succeeded; `MODULE.bazel.lock` remained unchanged.
  Divan was already in the workspace; only its TUI dev-dependency edge was added.

Run from `codex-rs` (nextest recipe supplies the local profile and 8 MiB stack):

```sh
cargo build --locked -p codex-tui --bin codex-tui \
  -p codex-file-search --bin codex-file-search -j 2
just test -p codex-tui --build-jobs 1 \
  --config 'profile.dev.package.codex-tui.debug=0' \
  --config 'profile.dev.package.codex-tui.codegen-units=256'
just test -p codex-file-search --build-jobs 1
just test -p codex-tui --lib --build-jobs 1 \
  --config 'profile.dev.package.codex-tui.debug=0' \
  --config 'profile.dev.package.codex-tui.codegen-units=256' \
  --run-ignored only --nocapture run_baseline_benchmarks
python3 tui/scripts/baseline_terminal.py --binary target/debug/codex-tui \
  --output ../.hoplite/artifacts/baseline/terminal
just fix -p codex-tui
just fmt
```

The ignored Divan driver avoids exporting benchmark-only product APIs. Use the
same compiler, profile, codegen settings, fixture, dimensions and host for future
before/after comparisons. Do not compare these numbers with release builds. Run
measurements without compilation or other test suites running concurrently.

## Measured baseline

Terminal probe: five fresh isolated homes per size, warm OS cache, then 20
single-key samples following an untimed Unicode draft. Times include tmux
launch/send/capture overhead and a 5 ms observation poll. No model turn is
submitted; the API endpoint is a closed localhost port. Startup can show the
expected background connection warning. "First frame" means the first usable
composer, not completion of all startup work. The resize smoke preserves the
Unicode draft and confirms subsequent input at all three sizes. This optional
Unix runner requires Python 3.11+ and tmux.

| Viewport | First composer median / p95 | Key-to-observed-frame median / p95 |
| --- | --- | --- |
| 80×24 | 297.803 / 380.023 ms | 52.742 / 226.518 ms |
| 120×40 | 235.321 / 240.397 ms | 58.207 / 137.450 ms |
| 200×50 | 267.133 / 425.452 ms | 68.631 / 369.925 ms |

Divan: 20 single-thread samples, existing private component APIs, no concurrent
build. Fixture construction is untimed; syntax/theme state is warm.

| Component | Median |
| --- | --- |
| `desired_height`, fresh cell, 80 / 120 / 200 columns | 3.846 / 3.651 / 3.605 ms |
| `desired_height`, reused cell, 80 / 120 / 200 columns | 3.804 / 3.670 / 3.623 ms |
| Markdown, 80 / 120 / 200 columns | 15.310 / 15.300 / 15.290 ms |
| Highlight 128 bounded Rust lines | 90.290 ms |
| File search, fresh session lifecycle | 23.000 ms |
| File search, warm alternating query | 0.611 ms |
| Git branch subprocess (not executor round trip) | 2.359 ms |
| Git porcelain-v2 status subprocess, 1,000 source files | 3.249 ms |
| Git executor round trip | **Pending**, local and remote must be distinguished |

Raw terminal, Divan and subprocess results are retained in [`baseline/`](baseline/).
Subprocess probes use an isolated unborn `baseline` branch, 20 warm samples, a 2 s
timeout, no concurrent build, and controlled output below 64 KiB. Their launch/
capture cost is included. Git uses `branch --show-current` and
`status --porcelain=v2 -z --branch --untracked-files=all` with fsmonitor/hooks disabled
for status. CLI cold-index search on that fixture is 28.439 ms median, distinct
from the in-process 256-source Divan fixture and from warm-session query latency.

Fresh/reused cells do not imply cache misses/hits: this fixture uses already
rendered `AgentMessageCell` lines. Cold search means a new index, not a cold OS
cache, and includes acknowledged worker-owner shutdown. Its fixture has 256
source files, 32 ignored files, a 4-byte binary and an 8 MiB text file. Untimed
preflight verifies ignored-path exclusion and discovery of both special files;
this does not read their contents or prove bounded reading/retrieval accuracy.

The initial 50 ms p95 input budget is not met by this observation harness. Do not
attribute its spikes to layout without tighter event/paint instrumentation.
Warm highlighting and markdown are component-cost candidates; debug-only numbers
are not production latency claims. Cold syntax initialization is still unmeasured.

## Snapshot evidence and hotspots

Six cases × 80×24, 120×40 and 200×50 produce 18 baseline snapshots: direct `Line`
clipping, Unicode prose, long URL/path/table, indented/tabbed code, 2,000 command
output lines, and multi-file add/update/delete diff. Tests use existing layout
and `HyperlinkParagraph` paint, recording logical widths and wrapped height.
OSC8 is stripped only from the final visual snapshot via the existing test helper;
production rendering and hyperlink metadata are unchanged. Existing hyperlink
remapping tests remain the source of link-alignment coverage.

Code-path findings, not an inferred CPU ranking:

1. `src/render/renderable.rs`: string/span/`Line` heights stay one; `Line` paints
   directly. The 436-column path repro loses its important suffix at every size.
   Code fixture tabs survive logical layout but disappear during paragraph paint.
2. `src/markdown_render.rs` permits intrinsic width. Link metadata must follow
   wrapping through `src/terminal_hyperlinks.rs`. OSC8 destinations repeat per
   painted cell; the long-link fixture initially produced ~424 KiB of snapshot
   text before visual normalization. Layout/cache byte accounting must include metadata.
3. `src/diff_render.rs` already owns gutters and wrapping. Ordinary path `Line`s
   lack an explicit overflow policy. Reuse alignment rather than duplicate it.
4. `src/exec_cell/render.rs::output_lines` limits head/tail rows, not line width.
   The fixture retains 11 logical lines with 392-column maximum and reports 1,990
   omitted source lines. Finalized `CommandOutput::line_counts` rescans aggregate
   text; live output has a separate bounded representation.
5. `src/render/highlight.rs` already has theme revisions, streaming support and
   guards of 512 KiB / 10,000 lines / 4 KiB per line. Reuse these; plain fallback
   still needs width/work-budget policy.
6. `codex-file-search` has background workers and a reusable Nucleo index. Top-N
   bounds results, not index bytes/entries; its work queue is unbounded. Walker
   cancellation checks every 1,024 entries and matcher ticks are 10 ms, not latency
   guarantees. `src/file_search.rs` rejects stale sessions on CWD change but scans
   the TUI host, which is not necessarily the remote workspace host.
7. `src/app_server_session/fs.rs::fs_read_file_path` receives a complete base64 file.
   Protocol `v2/fs.rs` lacks read offset/range/cap and metadata size: not a lazy reader.
8. `src/workspace_command.rs` provides async argv-based local/remote `command/exec`
   with 5 s / 64 KiB defaults. Outputs lack a truncation flag; requests lack a
   process ID/cancellation handle. Dropping a future does not prove remote termination.
9. `src/branch_summary.rs` counts committed changes against a merge base, not
   working-tree status. `/diff` loads all tracked/untracked diffs, disables output
   caps and uses 30 s per command, without an aggregate deadline. Its untracked
   paths are newline-split. Reuse helper-suppression/fsmonitor security, not loading.
   `git-utils/src/status.rs` tests whether porcelain-v1 output is empty; there is no
   reusable typed porcelain-v2 parser in that seam.

## Gate and proposed budgets

Verification: **4,653 TUI tests passed** (8 skipped), **16 file-search tests
passed**, and the opt-in Divan driver passed. All 18 generated snapshots were
reviewed and accepted before the full, ordinary TUI run (no snapshot force-pass).
The terminal probe passed all size/input/resize cases. Full workspace tests were
not run. Scoped Clippy fix, `just fmt`, and Bazel lock regeneration passed. The
post-test code diff was formatting-only; tests were not rerun after fix/format.

Proposed p95 budgets: first composer 1 s, input 50 ms, visible-block layout/height
4 ms, warm discovery 100 ms, local Git metadata 250 ms. Future Git probes should
have a hard 2 s / 64 KiB cap; cancellation acknowledgement target is 100 ms.
Flag >10% median/p95 regression only after repeated comparable runs. These are
initial targets, not measured guarantees or wall-clock CI assertions.

Step 0 remains open for the Git executor measurement and cold syntax initialization.
Optimized measurements and tighter
input/paint instrumentation are needed before asserting product performance.
Retrieval goldens/context budgets belong to their own milestone: no reading-quality
claim is made here. No layout refactor or panels have started. Full workspace
tests require separate approval.
