use super::app::TuiState;

pub(super) fn insert_input(app: &mut TuiState, character: char) {
    let cursor = app.input_cursor.min(app.input.len());
    app.input.insert(cursor, character);
    app.input_cursor = cursor + character.len_utf8();
    app.sync_slash_popup();
}

pub(super) fn backspace_input(app: &mut TuiState) {
    let Some(previous) = previous_char_boundary(&app.input, app.input_cursor) else {
        return;
    };
    app.input.drain(previous..app.input_cursor);
    app.input_cursor = previous;
    app.sync_slash_popup();
}

pub(super) fn delete_input(app: &mut TuiState) {
    let Some(next) = next_char_boundary(&app.input, app.input_cursor) else {
        return;
    };
    app.input.drain(app.input_cursor..next);
    app.sync_slash_popup();
}

pub(super) fn move_input_cursor_left(app: &mut TuiState) {
    if let Some(previous) = previous_char_boundary(&app.input, app.input_cursor) {
        app.input_cursor = previous;
    }
}

pub(super) fn move_input_cursor_right(app: &mut TuiState) {
    if let Some(next) = next_char_boundary(&app.input, app.input_cursor) {
        app.input_cursor = next;
    }
}

pub(super) fn move_input_cursor_to_line_start(app: &mut TuiState) {
    app.input_cursor = app.input[..app.input_cursor]
        .rfind('\n')
        .map_or(0, |index| index + 1);
}

pub(super) fn move_input_cursor_to_line_end(app: &mut TuiState) {
    app.input_cursor += app.input[app.input_cursor..]
        .find('\n')
        .unwrap_or_else(|| app.input.len() - app.input_cursor);
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
