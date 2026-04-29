mod app;
mod commands;
mod composer;
mod events;
mod limits;
mod model_picker;
mod slash_command_popup;
mod translator_picker;
mod view;

use std::{io, time::Duration};

use app::TuiState;
use commands::{
    apply_model_selection, apply_translator_selection, connect, interrupt_turn, submit_input,
};
use composer::{
    backspace_input, delete_input, insert_input, move_input_cursor_left, move_input_cursor_right,
    move_input_cursor_to_line_end, move_input_cursor_to_line_start,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use events::drain_app_server_events;
use model_picker::ModelPicker;
use ratatui::{Terminal, backend::CrosstermBackend};
use slash_command_popup::SlashCommandPopup;
use tokio::runtime::Runtime;
use translator_picker::TranslatorPicker;
use view::draw;

const TICK_RATE: Duration = Duration::from_millis(50);
const EVENT_DRAIN_TIMEOUT: Duration = Duration::from_millis(1);

pub fn run() -> io::Result<()> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_with_terminal(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_with_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let runtime = Runtime::new().map_err(io::Error::other)?;
    runtime.block_on(run_loop(terminal))
}

async fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = TuiState::new();
    terminal.draw(|frame| draw(frame, &app))?;

    let mut client = connect(&mut app)
        .await
        .map_err(|error| io::Error::other(error.to_string()))?;

    loop {
        drain_app_server_events(&mut client, &mut app).await;
        terminal.draw(|frame| draw(frame, &app))?;

        if event::poll(TICK_RATE)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Esc if app.translator_picker.is_some() => {
                        if app
                            .translator_picker
                            .as_mut()
                            .is_some_and(TranslatorPicker::cancel_or_back)
                        {
                            app.translator_picker = None;
                            app.status = "Translator settings canceled.".to_owned();
                        } else {
                            app.status = "Select a translator and settings.".to_owned();
                        }
                    }
                    KeyCode::Esc if app.model_picker.is_some() => {
                        if app
                            .model_picker
                            .as_mut()
                            .is_some_and(ModelPicker::cancel_or_back)
                        {
                            app.model_picker = None;
                            app.status = "Model selection canceled.".to_owned();
                        } else {
                            app.status = "Select a model and effort.".to_owned();
                        }
                    }
                    KeyCode::Up if app.translator_picker.is_some() => {
                        if let Some(picker) = app.translator_picker.as_mut() {
                            picker.select_previous();
                        }
                    }
                    KeyCode::Up if app.model_picker.is_some() => {
                        if let Some(picker) = app.model_picker.as_mut() {
                            picker.select_previous();
                        }
                    }
                    KeyCode::Down if app.translator_picker.is_some() => {
                        if let Some(picker) = app.translator_picker.as_mut() {
                            picker.select_next();
                        }
                    }
                    KeyCode::Down if app.model_picker.is_some() => {
                        if let Some(picker) = app.model_picker.as_mut() {
                            picker.select_next();
                        }
                    }
                    KeyCode::Enter if app.translator_picker.is_some() => {
                        if let Some(selection) = app
                            .translator_picker
                            .as_mut()
                            .and_then(TranslatorPicker::accept)
                            && let Err(error) =
                                apply_translator_selection(&mut app, selection).await
                        {
                            app.status = error;
                        }
                    }
                    KeyCode::Enter if app.model_picker.is_some() => {
                        if let Some(selection) =
                            app.model_picker.as_mut().and_then(ModelPicker::accept)
                        {
                            apply_model_selection(&mut app, selection);
                        }
                    }
                    KeyCode::Char(character) if app.translator_picker.is_some() => {
                        if let Some(picker) = app.translator_picker.as_mut() {
                            if picker.is_api_key_input()
                                && !character.is_control()
                                && !key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                picker.push_api_key_char(character);
                                continue;
                            }

                            if let Some(number) = character.to_digit(10).map(|digit| digit as usize)
                                && let Some(selection) = picker.select_number(number)
                            {
                                if let Err(error) =
                                    apply_translator_selection(&mut app, selection).await
                                {
                                    app.status = error;
                                }
                                continue;
                            }
                        }
                    }
                    KeyCode::Char(character) if app.model_picker.is_some() => {
                        if let Some(number) = character.to_digit(10).map(|digit| digit as usize)
                            && let Some(selection) = app
                                .model_picker
                                .as_mut()
                                .and_then(|picker| picker.select_number(number))
                        {
                            apply_model_selection(&mut app, selection);
                        }
                    }
                    KeyCode::Backspace if app.translator_picker.is_some() => {
                        if let Some(picker) = app.translator_picker.as_mut() {
                            picker.pop_api_key_char();
                        }
                    }
                    _ if app.translator_picker.is_some() => {}
                    _ if app.model_picker.is_some() => {}
                    KeyCode::Esc if app.slash_popup.is_some() => {
                        app.slash_popup = None;
                        app.status = "Command hints dismissed.".to_owned();
                    }
                    KeyCode::Up if app.slash_popup.is_some() => {
                        if let Some(popup) = app.slash_popup.as_mut() {
                            popup.select_previous();
                        }
                    }
                    KeyCode::Down if app.slash_popup.is_some() => {
                        if let Some(popup) = app.slash_popup.as_mut() {
                            popup.select_next();
                        }
                    }
                    KeyCode::Char('p')
                        if app.slash_popup.is_some()
                            && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        if let Some(popup) = app.slash_popup.as_mut() {
                            popup.select_previous();
                        }
                    }
                    KeyCode::Char('n')
                        if app.slash_popup.is_some()
                            && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        if let Some(popup) = app.slash_popup.as_mut() {
                            popup.select_next();
                        }
                    }
                    KeyCode::Tab if app.slash_popup.is_some() => {
                        complete_slash_command(&mut app);
                    }
                    KeyCode::Char('/') if app.slash_popup.is_some() => {
                        complete_slash_command(&mut app);
                    }
                    KeyCode::Enter if app.slash_popup.is_some() => {
                        accept_slash_command(&mut app);
                        submit_input(&mut client, &mut app).await;
                        if app.should_exit {
                            break;
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if app.active_turn_id.is_some() {
                            interrupt_turn(&mut client, &mut app).await;
                        } else {
                            break;
                        }
                    }
                    KeyCode::Esc if app.input.is_empty() => break,
                    KeyCode::Esc => {
                        app.input.clear();
                        app.input_cursor = 0;
                        app.sync_slash_popup();
                    }
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        insert_input(&mut app, '\n');
                    }
                    KeyCode::Enter => {
                        submit_input(&mut client, &mut app).await;
                        if app.should_exit {
                            break;
                        }
                    }
                    KeyCode::Backspace => {
                        backspace_input(&mut app);
                    }
                    KeyCode::Delete => {
                        delete_input(&mut app);
                    }
                    KeyCode::Left => {
                        move_input_cursor_left(&mut app);
                    }
                    KeyCode::Right => {
                        move_input_cursor_right(&mut app);
                    }
                    KeyCode::Home => {
                        move_input_cursor_to_line_start(&mut app);
                    }
                    KeyCode::End => {
                        move_input_cursor_to_line_end(&mut app);
                    }
                    KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        insert_input(&mut app, character);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    let _ = client.shutdown().await;
    Ok(())
}

fn complete_slash_command(app: &mut TuiState) {
    let Some(completion) = app
        .slash_popup
        .as_ref()
        .and_then(SlashCommandPopup::completion_text)
    else {
        return;
    };

    app.input = completion;
    app.input_cursor = app.input.len();
    app.sync_slash_popup();
}

fn accept_slash_command(app: &mut TuiState) -> bool {
    let Some(command) = app
        .slash_popup
        .as_ref()
        .and_then(SlashCommandPopup::selected_command)
    else {
        return false;
    };

    app.input = format!("/{command}");
    app.input_cursor = app.input.len();
    app.slash_popup = None;
    true
}
