//! Opt-in component diagnostics, not first-frame, input-to-frame, or remote Git latency.
//! Run the ignored driver through `just test --release -p codex-tui --run-ignored only
//! run_baseline_benchmarks --nocapture`. Fixtures are built outside timed sections.
//! Fresh cells have no layout cache today; reused cells are not cache-hit measurements.
//! Search "cold" means a new index, not a cold OS filesystem cache. Contents are not read.

use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;
use crate::markdown_render::render_markdown_text_with_width_and_cwd;
use crate::render::highlight::highlight_code_to_lines;
use codex_file_search::FileSearchOptions;
use codex_file_search::FileSearchSession;
use codex_file_search::FileSearchSnapshot;
use codex_file_search::SessionReporter;
use codex_file_search::create_session;
use divan::Bencher;
use divan::black_box;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

fn markdown_fixture() -> String {
    format!(
        "# Reading baseline\n\n{}\n\nhttps://example.invalid/{}\n\n```rust\n{}\n```\n\n\
         | File | Status |\n| --- | --- |\n| src/main.rs | modified |\n",
        "Long prose with Unicode 日本語 and nested `code` for wrapping. ".repeat(/*n*/ 48),
        "long-segment/".repeat(/*n*/ 32),
        "\tlet value = compute(input); // preserve indentation\n".repeat(/*n*/ 32),
    )
}

#[divan::bench(args = [80, 120, 200])]
fn desired_height_fresh_cell(bencher: Bencher, width: u16) {
    let lines = render_markdown_text_with_width_and_cwd(
        &markdown_fixture(),
        Some(usize::from(width)),
        Some(Path::new(".")),
    )
    .lines;
    bencher
        .with_inputs(|| AgentMessageCell::new(lines.clone(), /*is_first_line*/ true))
        .bench_local_refs(|cell| cell.desired_height(black_box(width)));
}

#[divan::bench(args = [80, 120, 200])]
fn desired_height_reused_cell(bencher: Bencher, width: u16) {
    let lines = render_markdown_text_with_width_and_cwd(
        &markdown_fixture(),
        Some(usize::from(width)),
        Some(Path::new(".")),
    )
    .lines;
    let cell = AgentMessageCell::new(lines, /*is_first_line*/ true);
    black_box(cell.desired_height(width));
    bencher.bench_local(|| cell.desired_height(black_box(width)));
}

#[divan::bench(args = [80, 120, 200])]
fn markdown_render(bencher: Bencher, width: usize) {
    let source = markdown_fixture();
    bencher.bench_local(|| {
        render_markdown_text_with_width_and_cwd(
            black_box(&source),
            Some(width),
            Some(Path::new(".")),
        )
    });
}

#[divan::bench]
fn syntax_highlight_bounded_rust(bencher: Bencher) {
    let source = "\tlet result = values.iter().map(|value| value + 1).collect::<Vec<_>>();\n"
        .repeat(/*n*/ 128);
    // Initialize shared syntax/theme state before measuring steady-state highlighting.
    black_box(highlight_code_to_lines(&source, "rust"));
    bencher.bench_local(|| highlight_code_to_lines(black_box(&source), "rust"));
}

fn search_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("search fixture");
    fs::create_dir(dir.path().join(".git")).expect("Git ignore context");
    fs::create_dir(dir.path().join("ignored")).expect("ignored directory");
    fs::write(dir.path().join(".gitignore"), "ignored/\n").expect("ignore rules");
    for index in 0..256 {
        fs::write(
            dir.path().join(format!("source-{index:04}.rs")),
            "fn fixture() {}\n",
        )
        .expect("source fixture");
    }
    for index in 0..32 {
        fs::write(
            dir.path().join(format!("ignored/source-{index:04}.rs")),
            "ignored\n",
        )
        .expect("ignored fixture");
    }
    fs::write(dir.path().join("binary.bin"), [0, 255, 0, 128]).expect("binary fixture");
    let text_line = "0123456789abcde\n";
    fs::write(
        dir.path().join("large.txt"),
        text_line.repeat(/*n*/ 8 * 1024 * 1024 / text_line.len()),
    )
    .expect("8 MiB text fixture");
    dir
}

enum SearchEvent {
    Snapshot(FileSearchSnapshot),
    Complete,
    Stopped,
}

struct SearchReporter(mpsc::SyncSender<SearchEvent>);

impl SessionReporter for SearchReporter {
    fn on_update(&self, snapshot: &FileSearchSnapshot) {
        let _ = self.0.send(SearchEvent::Snapshot(snapshot.clone()));
    }

    fn on_complete(&self) {
        let _ = self.0.send(SearchEvent::Complete);
    }
}

impl Drop for SearchReporter {
    fn drop(&mut self) {
        let _ = self.0.send(SearchEvent::Stopped);
    }
}

struct SearchProbe {
    session: Option<FileSearchSession>,
    events: mpsc::Receiver<SearchEvent>,
    cancelled: Arc<AtomicBool>,
}

impl SearchProbe {
    fn new(root: &Path) -> Self {
        let (sender, events) = mpsc::sync_channel(/*bound*/ 8);
        let cancelled = Arc::new(AtomicBool::new(/*v*/ false));
        let session = create_session(
            vec![root.to_path_buf()],
            FileSearchOptions {
                compute_indices: true,
                ..Default::default()
            },
            Arc::new(SearchReporter(sender)),
            Some(cancelled.clone()),
        )
        .expect("search session");
        Self {
            session: Some(session),
            events,
            cancelled,
        }
    }

    fn query(&self, query: &str) -> FileSearchSnapshot {
        while self.events.try_recv().is_ok() {}
        self.session
            .as_ref()
            .expect("live session")
            .update_query(query);
        let deadline = Instant::now() + Duration::from_secs(/*secs*/ 5);
        let mut latest = None;
        loop {
            match self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(SearchEvent::Snapshot(snapshot)) => {
                    if snapshot.query == query {
                        latest = Some(snapshot);
                    }
                }
                Ok(SearchEvent::Complete) => {
                    // Completion requires an idle matcher and finished walk; the last
                    // changed snapshot need not itself have walk_complete set.
                    if let Some(snapshot) = latest {
                        return snapshot;
                    }
                }
                Ok(SearchEvent::Stopped) => panic!("search stopped before query completed"),
                Err(error) => {
                    self.cancelled.store(/*val*/ true, Ordering::Relaxed);
                    panic!("search baseline did not finish before deadline: {error}");
                }
            }
        }
    }
}

impl Drop for SearchProbe {
    fn drop(&mut self) {
        self.cancelled.store(/*val*/ true, Ordering::Relaxed);
        self.session.take();
        let deadline = Instant::now() + Duration::from_secs(/*secs*/ 5);
        loop {
            match self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(SearchEvent::Stopped) => break,
                Ok(SearchEvent::Snapshot(_) | SearchEvent::Complete) => {}
                Err(error) => {
                    if !std::thread::panicking() {
                        panic!("search worker owners did not release reporter: {error}");
                    }
                    break;
                }
            }
        }
    }
}

fn verify_search_fixture(root: &Path) {
    let probe = SearchProbe::new(root);
    assert!(probe.query("ignored").matches.is_empty());
    for name in ["binary.bin", "large.txt"] {
        let snapshot = probe.query(name);
        assert_eq!(
            snapshot
                .matches
                .iter()
                .map(|entry| entry.path.as_path())
                .collect::<Vec<_>>(),
            vec![Path::new(name)],
        );
    }
    assert_eq!(probe.query("source-00").matches.len(), 20);
    assert_eq!(probe.query("source-01").matches.len(), 20);
}

#[divan::bench(sample_size = 1)]
fn file_search_cold_session_lifecycle(bencher: Bencher) {
    let dir = search_fixture();
    verify_search_fixture(dir.path());
    // Include shutdown acknowledgement so consecutive samples cannot overlap worker owners.
    bencher.bench_local(|| SearchProbe::new(dir.path()).query("source-00"));
}

#[divan::bench(sample_size = 1)]
fn file_search_warm_query(bencher: Bencher) {
    let dir = search_fixture();
    verify_search_fixture(dir.path());
    let probe = SearchProbe::new(dir.path());
    black_box(probe.query("source-00"));
    let mut alternate = false;
    bencher.bench_local(|| {
        alternate = !alternate;
        probe.query(if alternate { "source-01" } else { "source-00" })
    });
}

#[test]
#[ignore = "opt-in release component benchmarks; not end-to-end latency"]
fn run_baseline_benchmarks() {
    divan::Divan::default()
        .sample_count(/*count*/ 20)
        .threads([1])
        .run_benches();
}
