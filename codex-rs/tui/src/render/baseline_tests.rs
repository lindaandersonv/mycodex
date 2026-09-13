//! Characterize current transcript wrapping, including oversized logical lines and clipping.

use crate::diff_model::FileChange;
use crate::diff_render::create_diff_summary;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::OutputLinesParams;
use crate::exec_cell::TOOL_CALL_MAX_LINES;
use crate::exec_cell::output_lines;
use crate::markdown_render::render_markdown_lines_with_width_and_cwd;
use crate::render::renderable::Renderable;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::HyperlinkParagraph;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::terminal_hyperlinks::strip_osc8;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

fn snapshot_sizes(
    scenario: &str,
    source: &str,
    notes: &str,
    layout: impl Fn(u16) -> Vec<HyperlinkLine>,
) {
    for (width, height) in [(80, 24), (120, 40), (200, 50)] {
        let lines = layout(width);
        let widths: Vec<_> = lines.iter().map(|line| line.line.width()).collect();
        let oversized: Vec<_> = widths
            .iter()
            .enumerate()
            .filter(|(_, columns)| **columns > usize::from(width))
            .map(|(index, columns)| (index + 1, *columns))
            .collect();
        let logical_tabs = lines
            .iter()
            .flat_map(|line| &line.line.spans)
            .map(|span| span.content.matches('\t').count())
            .sum::<usize>();
        let paragraph = HyperlinkParagraph::new(&lines, Style::default());
        let rows = paragraph.line_count(width);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| paragraph.render(frame.area(), frame.buffer_mut()))
            .expect("draw baseline");

        // Widths describe logical lines before the real viewport's secondary wrapping.
        // Oversized lines are not evidence of buffer overflow or a new scroll policy.
        let diagnostics = format!(
            "scenario={scenario} viewport={width}x{height}\n\
             source_bytes={} source_lines={} source_tabs={}\n\
             logical_lines={} max_logical_columns={} logical_tabs={}\n\
             logical_lines_wider_than_viewport={} first_8_line_number_and_width={:?}\n\
             measured_wrapped_rows={rows} rows_below_viewport={}\n\
             paint=HyperlinkParagraph wrap(trim=false), top viewport, no horizontal scroll\n\
             {notes}\n\n{}",
            source.len(),
            source.lines().count(),
            source.matches('\t').count(),
            lines.len(),
            widths.iter().max().copied().unwrap_or_default(),
            logical_tabs,
            oversized.len(),
            oversized.iter().take(/*n*/ 8).collect::<Vec<_>>(),
            rows.saturating_sub(usize::from(height)),
            strip_osc8(&terminal.backend().to_string()),
        );
        insta::assert_snapshot!(format!("{scenario}_{width}x{height}"), diagnostics);
    }
}

fn snapshot_markdown(scenario: &str, source: &str) {
    snapshot_sizes(
        scenario,
        source,
        "layout=existing markdown writer",
        |width| {
            render_markdown_lines_with_width_and_cwd(
                source,
                Some(usize::from(width)),
                Some(Path::new(".")),
            )
        },
    );
}

#[test]
fn long_prose_unicode() {
    let paragraph = "Read the surrounding code before changing behavior. \
        日本語の説明と中文内容を確認します。 Cafe\u{301} and nai\u{308}ve preserve combining marks. ";
    let source = format!(
        "# Reading baseline\n\n{}\n\n- {}\n\n> {}\n",
        paragraph.repeat(/*n*/ 12),
        paragraph.repeat(/*n*/ 8),
        paragraph.repeat(/*n*/ 6),
    );
    snapshot_markdown("long_prose_unicode", &source);
}

#[test]
fn long_url_and_path() {
    let url = format!(
        "https://example.invalid/{}/reference?revision=baseline#L120",
        "long-segment-".repeat(/*n*/ 24),
    );
    let path = format!(
        "src/{}/component.rs:120:8",
        "nested-directory/".repeat(/*n*/ 24)
    );
    let source = format!(
        "# URL and path baseline\n\n\
         The complete URL follows: {url}\n\n\
         [{url}]({url})\n\n\
         The complete path follows: `{path}`\n\n\
         | Kind | Location | Explanation |\n\
         | --- | --- | --- |\n\
         | source | `{path}` | Keep the narrative readable beside a long path. |\n"
    );
    snapshot_markdown("long_url_and_path", &source);
}

#[test]
fn long_code_indentation_and_tabs() {
    let long_value = "preserve_spaces_and_full_line_".repeat(/*n*/ 14);
    let source = format!(
        "# Code baseline\n\n```rust\nfn main() {{\n\
         \tlet tab_indented = \"{long_value}\";\n\
         \x20\x20\x20\x20let space_indented = \"{long_value}\";\n\
         \t\tprintln!(\"{{tab_indented}} {{space_indented}}\");\n}}\n```\n\n\
         ```text\n\talpha\tbeta\t日本語\n        eight_spaces_then_{long_value}\n```\n"
    );
    snapshot_markdown("long_code_indentation_and_tabs", &source);
}

#[test]
fn large_command_output() {
    let source: String = (0..2_000)
        .map(|index| {
            format!(
                "{index:04} \u{1b}[32mOK\u{1b}[0m {} 日本語 cafe\u{301}\n",
                "command-output-segment ".repeat(/*n*/ 16),
            )
        })
        .collect();
    let output = CommandOutput::new(/*exit_code*/ 0, source.clone());
    let rendered = output_lines(
        Some(&output),
        OutputLinesParams {
            line_limit: TOOL_CALL_MAX_LINES,
            only_err: false,
            include_angle_pipe: true,
            include_prefix: true,
        },
    );
    let notes = format!(
        "layout=existing output_lines; omitted_source_lines={:?}; not the live-output byte-cap path",
        rendered.omitted,
    );
    snapshot_sizes("large_command_output", &source, &notes, |_| {
        plain_hyperlink_lines(rendered.lines.clone())
    });
}

#[test]
fn multi_file_diff() {
    let before = "fn main() {\n    old_call();\n}\n";
    let after = format!(
        "fn main() {{\n\tlet value = \"{}\";\n    new_call(value);\n}}\n",
        "long_diff_value_".repeat(/*n*/ 24),
    );
    let patch = diffy::create_patch(before, &after).to_string();
    let added = "first line\n\tindented 日本語 cafe\u{301}\nlast line\n";
    let deleted = "obsolete entry\n";
    let changes = HashMap::from([
        (
            PathBuf::from("src/main.rs"),
            FileChange::Update {
                unified_diff: patch.clone(),
                move_path: None,
            },
        ),
        (
            PathBuf::from(format!(
                "notes/{}added.txt",
                "long-directory/".repeat(/*n*/ 18)
            )),
            FileChange::Add {
                content: added.to_string(),
            },
        ),
        (
            PathBuf::from("obsolete.txt"),
            FileChange::Delete {
                content: deleted.to_string(),
            },
        ),
    ]);
    let source = format!("{patch}{added}{deleted}");
    snapshot_sizes(
        "multi_file_diff",
        &source,
        "layout=existing create_diff_summary; source diagnostics cover patch/add/delete text",
        |width| {
            plain_hyperlink_lines(create_diff_summary(
                &changes,
                Path::new("."),
                usize::from(width),
            ))
        },
    );
}

#[test]
fn intrinsic_line_clipping() {
    let path = format!(
        "src/{}/important_suffix.rs:120",
        "nested-directory/".repeat(/*n*/ 24)
    );
    let line = Line::from(path);
    for (width, height) in [(80, 24), (120, 40), (200, 50)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| Renderable::render(&line, frame.area(), frame.buffer_mut()))
            .expect("draw intrinsic line");
        let snapshot = format!(
            "viewport={width}x{height} logical_columns={} desired_height={}\n\
             paint=Line Renderable; implicit clipping, no wrap or horizontal scroll\n\n{}",
            line.width(),
            line.desired_height(width),
            terminal.backend(),
        );
        insta::assert_snapshot!(
            format!("intrinsic_line_clipping_{width}x{height}"),
            snapshot
        );
    }
}
