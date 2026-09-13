//! Explicit word wrapping for prose headings. Paint never wraps a second time.

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use crate::line_truncation::line_width;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_line;

use super::renderable::Renderable;

/// Opt-in prose: raw code, paths and tables must retain their own overflow policies.
pub(crate) struct Prose {
    line: Line<'static>,
}

impl Prose {
    pub(crate) fn new(line: Line<'static>) -> Self {
        Self { line }
    }
}

impl Renderable for Prose {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        ProseLayout::new(&self.line, area.width).paint(area, buf, /*scroll_rows*/ 0);
    }

    fn desired_height(&self, width: u16) -> u16 {
        ProseLayout::new(&self.line, width).height()
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        ProseLayout::new(&self.line, area.width).paint(area, buf, scroll_offset);
        true
    }
}

pub(super) struct ProseLayout<'a> {
    rows: Cow<'a, [Line<'a>]>,
}

impl<'a> ProseLayout<'a> {
    pub(super) fn new(line: &'a Line<'a>, width: u16) -> Self {
        let width = usize::from(width);
        let rows = if width == 0 {
            Cow::Borrowed(&[][..])
        } else if line_width(line) <= width {
            Cow::Borrowed(std::slice::from_ref(line))
        } else {
            let indent = line
                .spans
                .iter()
                .flat_map(|span| span.content.chars())
                .take_while(|ch| *ch == ' ')
                .count()
                .min(width.saturating_sub(1));
            let options =
                RtOptions::new(width).subsequent_indent(Line::from(" ".repeat(/*n*/ indent)));
            let mut rows = word_wrap_line(line, options);
            for row in &mut rows {
                row.alignment = line.alignment;
                // A single grapheme can be wider than a one-column viewport.
                if line_width(row) > width {
                    let style = row
                        .spans
                        .last()
                        .map_or(row.style, |span| row.style.patch(span.style));
                    *row = Line::from("…").style(style);
                }
            }
            Cow::Owned(rows)
        };
        Self { rows }
    }

    pub(super) fn height(&self) -> u16 {
        self.rows.len().try_into().unwrap_or(u16::MAX)
    }

    pub(super) fn paint(&self, area: Rect, buf: &mut Buffer, scroll_rows: u16) {
        for (row, y) in self
            .rows
            .iter()
            .skip(usize::from(scroll_rows))
            .zip(area.rows())
        {
            Widget::render(row, y, buf);
        }
    }
}

#[cfg(test)]
#[path = "prose_tests.rs"]
mod tests;
