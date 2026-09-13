use super::Prose;
use super::ProseLayout;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::borrow::Cow;

#[test]
fn short_text_borrows_the_original_styled_line() {
    let line = Line::from(vec!["read ".bold(), "src/main.rs".cyan()]).right_aligned();
    let layout = ProseLayout::new(&line, /*width*/ 80);
    assert!(matches!(layout.rows, Cow::Borrowed(_)));
    assert_eq!(layout.rows.as_ref(), std::slice::from_ref(&line));
}

#[test]
fn continuation_keeps_indent_and_styles() {
    let line = Line::from(vec!["  ".into(), "alpha beta gamma".cyan()]);
    let layout = ProseLayout::new(&line, /*width*/ 12);
    assert_eq!(
        layout.rows.as_ref(),
        &[
            Line::from(vec!["  ".into(), "alpha beta".cyan()]),
            Line::from(vec!["  ".into(), "gamma".cyan()]),
        ]
    );
}

#[test]
fn empty_and_zero_width_layouts_have_consistent_heights() {
    let empty = Line::default();
    let text = Line::from("text");
    assert_eq!(
        [
            ProseLayout::new(&empty, /*width*/ 80).height(),
            ProseLayout::new(&empty, /*width*/ 0).height(),
            ProseLayout::new(&text, /*width*/ 0).height(),
        ],
        [1, 0, 0]
    );
}

#[test]
fn unicode_and_overlong_tokens_are_bounded_at_every_width() {
    for source in [
        "  日本語 cafe\u{301} 👩‍💻 ｶﾞ accuracy",
        "https://example.invalid/long/segment?revision=abcdef#L120",
        "src/very-long-directory-name/important_suffix.rs:120",
        "                         indented",
    ] {
        let line = Line::from(source);
        for width in 0..=80 {
            let layout = ProseLayout::new(&line, width);
            assert!(
                layout
                    .rows
                    .iter()
                    .all(|row| row.width() <= usize::from(width))
            );
        }
    }
    let wide = Line::from("界".red());
    assert_eq!(
        ProseLayout::new(&wide, /*width*/ 1).rows.as_ref(),
        &[Line::from("…").red()]
    );
}

#[test]
fn paint_uses_measured_rows_and_respects_the_viewport() {
    let line = Line::from("alpha beta gamma delta");
    let layout = ProseLayout::new(&line, /*width*/ 10);
    assert_eq!(layout.height(), 3);
    let mut buffer = Buffer::with_lines(["##############"; 5]);
    let area = Rect::new(
        /*x*/ 2, /*y*/ 1, /*width*/ 10, /*height*/ 2,
    );
    layout.paint(area, &mut buffer, /*scroll_rows*/ 1);
    assert_eq!(
        buffer,
        Buffer::with_lines([
            "##############",
            "##gamma#######",
            "##delta#######",
            "##############",
            "##############",
        ])
    );
}

#[test]
fn prose_renderable_uses_the_same_layout_for_height_paint_and_scroll() {
    let source = "alpha beta gamma delta";
    let renderable = Prose::new(Line::from(source));
    let expected = Buffer::with_lines(["alpha beta", "gamma     ", "delta     "]);
    let width = expected.area.width;
    assert_eq!(renderable.desired_height(width), expected.area.height);
    let mut buffer = Buffer::empty(expected.area);
    renderable.render(expected.area, &mut buffer);
    assert_eq!(buffer, expected);
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 2);
    let mut buffer = Buffer::empty(area);
    assert!(renderable.render_scrolled(area, &mut buffer, /*scroll_offset*/ 1));
    assert_eq!(buffer, Buffer::with_lines(["gamma     ", "delta     "]));
}

#[test]
fn column_places_the_next_child_below_all_wrapped_rows() {
    let column = ColumnRenderable::with([
        Box::new(Prose::new(Line::from("alpha beta gamma delta"))) as Box<dyn Renderable>,
        Box::new(Line::from("next")),
    ]);
    let area = Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        /*width*/ 10,
        column.desired_height(/*width*/ 10),
    );
    let mut buffer = Buffer::empty(area);
    column.render(area, &mut buffer);
    assert_eq!(
        buffer,
        Buffer::with_lines(["alpha beta", "gamma     ", "delta     ", "next      "])
    );
}

#[test]
fn prose_snapshots_narrow_and_wide() {
    let line = Line::from(vec![
        "  Read carefully: ".bold(),
        "日本語 cafe\u{301} and Unicode text remain scannable. ".into(),
        "src/long-directory-name/important_suffix.rs:120".cyan(),
    ]);
    let snapshots: Vec<_> = [16, 80, 200]
        .into_iter()
        .map(|width| {
            let layout = ProseLayout::new(&line, width);
            let area = Rect::new(/*x*/ 0, /*y*/ 0, width, layout.height());
            let mut buffer = Buffer::empty(area);
            layout.paint(area, &mut buffer, /*scroll_rows*/ 0);
            format!("{buffer:?}")
        })
        .collect();
    insta::assert_snapshot!("prose_narrow_and_wide", snapshots.join("\n\n"));
}
