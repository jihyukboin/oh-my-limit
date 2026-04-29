use std::ops::Range;

use ratatui::layout::Rect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Default)]
pub(super) struct TextArea {
    text: String,
    cursor: usize,
}

impl TextArea {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.text.len();
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub(super) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(super) fn insert_char(&mut self, character: char) {
        let cursor = self.cursor.min(self.text.len());
        self.text.insert(cursor, character);
        self.cursor = cursor + character.len_utf8();
    }

    pub(super) fn insert_text(&mut self, text: &str) {
        let cursor = self.cursor.min(self.text.len());
        self.text.insert_str(cursor, text);
        self.cursor = cursor + text.len();
    }

    pub(super) fn backspace(&mut self) {
        let Some(previous) = previous_char_boundary(&self.text, self.cursor) else {
            return;
        };
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
    }

    pub(super) fn delete(&mut self) {
        let Some(next) = next_char_boundary(&self.text, self.cursor) else {
            return;
        };
        self.text.drain(self.cursor..next);
    }

    pub(super) fn move_cursor_left(&mut self) {
        if let Some(previous) = previous_char_boundary(&self.text, self.cursor) {
            self.cursor = previous;
        }
    }

    pub(super) fn move_cursor_right(&mut self) {
        if let Some(next) = next_char_boundary(&self.text, self.cursor) {
            self.cursor = next;
        }
    }

    pub(super) fn move_cursor_to_line_start(&mut self) {
        self.cursor = self.text[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
    }

    pub(super) fn move_cursor_to_line_end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or_else(|| self.text.len() - self.cursor);
    }

    pub(super) fn visual_line_count(&self, width: u16) -> u16 {
        self.wrapped_ranges(width)
            .len()
            .try_into()
            .unwrap_or(u16::MAX)
    }

    pub(super) fn visible_lines(&self, width: u16, height: u16) -> Vec<String> {
        let ranges = self.wrapped_ranges(width);
        let scroll = self.effective_scroll(height, &ranges);
        ranges
            .iter()
            .skip(scroll)
            .take(height as usize)
            .map(|range| self.text.get(range.clone()).unwrap_or_default().to_owned())
            .collect()
    }

    pub(super) fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let ranges = self.wrapped_ranges(area.width);
        let scroll = self.effective_scroll(area.height, &ranges);
        let cursor_line = self.cursor_visual_line(&ranges);
        let line = ranges
            .get(cursor_line)
            .cloned()
            .unwrap_or(self.cursor..self.cursor);
        let column_text = self
            .text
            .get(line.start..self.cursor.min(line.end))
            .unwrap_or("");
        let column = column_text.width() as u16;

        (
            area.x
                .saturating_add(column.min(area.width.saturating_sub(1))),
            area.y.saturating_add(
                cursor_line
                    .saturating_sub(scroll)
                    .min(area.height as usize - 1) as u16,
            ),
        )
    }

    fn effective_scroll(&self, height: u16, ranges: &[Range<usize>]) -> usize {
        if height == 0 || ranges.len() <= height as usize {
            return 0;
        }

        let cursor_line = self.cursor_visual_line(ranges);
        cursor_line
            .saturating_add(1)
            .saturating_sub(height as usize)
    }

    fn cursor_visual_line(&self, ranges: &[Range<usize>]) -> usize {
        ranges
            .iter()
            .position(|range| {
                (range.start..=range.end).contains(&self.cursor)
                    || (range.start == range.end && self.cursor == range.start)
            })
            .unwrap_or_else(|| ranges.len().saturating_sub(1))
    }

    fn wrapped_ranges(&self, width: u16) -> Vec<Range<usize>> {
        let width = usize::from(width).max(1);
        if self.text.is_empty() {
            return std::iter::once(0..0).collect();
        }

        let mut ranges = Vec::new();
        let mut line_start = 0;
        for segment in self.text.split_inclusive('\n') {
            let segment_start = line_start;
            let content = segment.strip_suffix('\n').unwrap_or(segment);
            self.push_wrapped_segment_ranges(segment_start, content, width, &mut ranges);
            line_start += segment.len();
            if segment.ends_with('\n') {
                ranges.push(line_start..line_start);
            }
        }

        ranges
    }

    fn push_wrapped_segment_ranges(
        &self,
        segment_start: usize,
        segment: &str,
        width: usize,
        ranges: &mut Vec<Range<usize>>,
    ) {
        if segment.is_empty() {
            ranges.push(segment_start..segment_start);
            return;
        }

        let mut start = segment_start;
        let mut current_width = 0;
        for (offset, character) in segment.char_indices() {
            let char_width = character.width().unwrap_or_default();
            let absolute = segment_start + offset;
            if current_width > 0 && current_width + char_width > width {
                ranges.push(start..absolute);
                start = absolute;
                current_width = 0;
            }
            current_width += char_width;
        }
        ranges.push(start..segment_start + segment.len());
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }

    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }

    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| cursor + index)
        .or(Some(text.len()))
}
