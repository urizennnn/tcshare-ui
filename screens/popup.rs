use crate::home::homepage::Home;
use derive_setters::Setters;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::{layout::Rect, text::Line, widgets::Clear, Frame};
use tui_confirm_dialog::{ButtonLabel, ConfirmDialog, ConfirmDialogState};
use tui_textarea::TextArea;

#[derive(Default, Setters)]
pub struct ApiPopup<'a> {
    pub title: String,
    pub message: String,
    pub input: Block<'a>,
    pub buttons: Vec<String>,
}

impl Widget for ApiPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let outer_block = Block::default()
            .title(self.title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ratatui::style::Color::Yellow));
        outer_block.render(area, buf);

        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let centered_message = Paragraph::new(self.message)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true });
        centered_message.render(chunks[0], buf);

        let mut input = TextArea::default();
        input.set_block(Block::default().borders(Borders::ALL));
        input.render(chunks[1], buf);
    }
}

impl<'a> ApiPopup<'a> {
    pub fn new() -> Self {
        Self::default()
    }
}

pub(crate) static mut FLAG: bool = false;

#[derive(Debug)]
pub struct InputBox {
    pub input: String,
    pub character_index: usize,
    pub input_mode: InputMode,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum InputMode {
    Normal,
    Editing,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            input_mode: InputMode::Normal,
            character_index: 0,
            input: String::new(),
        }
    }

    pub(crate) fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    pub(crate) fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    pub(crate) fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
    }

    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    pub(crate) fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.input.chars().skip(current_index);

            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    pub(crate) fn submit_message(&mut self) -> String {
        let input_msg = self.input.clone();
        self.input.clear();
        self.reset_cursor();
        input_msg
    }

    pub fn draw(&self, frame: &mut Frame) {
        let input_mode = if unsafe { FLAG } {
            InputMode::Editing
        } else {
            InputMode::Normal
        };

        let vertical = Layout::vertical([Constraint::Length(1), Constraint::Length(3)]);
        let [help_area, input_area] = vertical.areas(frame.area());

        let (msg, style) = match input_mode {
            InputMode::Normal => (
                vec![
                    "Press ".into(),
                    "Esc".bold(),
                    " to exit, ".into(),
                    "e".bold(),
                    " to start editing.".bold(),
                ],
                Style::default().add_modifier(Modifier::RAPID_BLINK),
            ),
            InputMode::Editing => (
                vec![
                    "Press ".into(),
                    "q".bold(),
                    " to stop editing, ".into(),
                    "Enter".bold(),
                    " to record the message".into(),
                ],
                Style::default(),
            ),
        };
        let text = Text::from(Line::from(msg)).patch_style(style);
        let help_message = Paragraph::new(text);
        frame.render_widget(Clear, help_area);
        frame.render_widget(help_message, help_area);

        let input = Paragraph::new(self.input.as_str())
            .style(match input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(ratatui::style::Color::Yellow),
            })
            .block(Block::default().borders(Borders::ALL).title("Input"));
        frame.render_widget(input, input_area);

        if input_mode == InputMode::Editing {
            frame.set_cursor_position(Position::new(
                input_area.x + self.character_index as u16 + 1,
                input_area.y + 1,
            ));
        }
    }
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Home {
    pub fn render_notification(&mut self, frame: &mut Frame) {
        self.popup_dialog = ConfirmDialogState::default()
            .modal(true)
            .with_title(Span::styled("Notification", Style::new().bold().cyan()))
            .with_text(vec![Line::from("Are you an admin?")])
            .with_yes_button(ButtonLabel::from("(Y)es").unwrap())
            .with_no_button(ButtonLabel::from("(N)o").unwrap())
            .with_yes_button_selected(self.selected_button == 0)
            .with_listener(Some(self.popup_tx.clone()))
            .open();

        let area = self.calculate_popup_area(frame.area(), 50, 30);

        if self.popup_dialog.is_opened() {
            let popup = ConfirmDialog::default()
                .borders(Borders::ALL)
                .bg(ratatui::style::Color::Black)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .button_style(ratatui::prelude::Style::default())
                .selected_button_style(
                    ratatui::prelude::Style::default()
                        .fg(ratatui::style::Color::Yellow)
                        .bold(),
                );

            frame.render_widget(Clear, area);
            frame.render_stateful_widget(popup, area, &mut self.popup_dialog);
        }
    }

    fn calculate_popup_area(&self, area: Rect, percent_x: u16, percent_y: u16) -> Rect {
        let popup_width = area.width * percent_x / 100;
        let popup_height = area.height * percent_y / 100;

        let popup_x = (area.width - popup_width) / 2;
        let popup_y = (area.height - popup_height) / 2;

        Rect::new(
            area.x + popup_x,
            area.y + popup_y,
            popup_width,
            popup_height,
        )
    }
}
