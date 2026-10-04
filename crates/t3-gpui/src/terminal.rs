//! The remote terminal's screen: output runs through a VT100 parser into a
//! grid of cells, painted row by row, and keystrokes become the bytes a
//! terminal would send, so the shell edits its own prompt in place.

use std::ops::Range;

use gpui_kit::*;

const SCROLLBACK: usize = 5_000;

pub struct TerminalScreen {
    parser: vt100::Parser,
}

impl Default for TerminalScreen {
    fn default() -> Self {
        Self { parser: vt100::Parser::new(24, 80, SCROLLBACK) }
    }
}

/// How the screen is painted: one monospace cell per column.
pub struct TerminalStyle {
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub cell: Size<Pixels>,
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
}

impl TerminalScreen {
    /// `(rows, cols)`.
    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    pub fn process(&mut self, data: &str) {
        self.parser.process(data.as_bytes());
    }

    /// Starts over with an empty screen of the same size.
    pub fn reset(&mut self) {
        let (rows, cols) = self.size();
        self.parser = vt100::Parser::new(rows, cols, SCROLLBACK);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Moves the view `lines` into the scrollback (negative: back toward
    /// the live screen).
    pub fn scroll(&mut self, lines: i32) {
        let offset = self.parser.screen().scrollback() as i64 + lines as i64;
        self.parser.screen_mut().set_scrollback(offset.max(0) as usize);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }

    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    /// The bytes a terminal sends for `keystroke`, if any.
    pub fn input_for(&self, keystroke: &Keystroke) -> Option<String> {
        keystroke_input(keystroke, self.parser.screen().application_cursor())
    }

    pub fn render(&self, focused: bool, style: &TerminalStyle) -> impl IntoElement {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let cursor = (screen.scrollback() == 0 && !screen.hide_cursor())
            .then(|| screen.cursor_position());
        let lines = (0..rows).map(|row| {
            let (text, highlights) = row_runs(screen, row, cols, style);
            div()
                .h(style.cell.height)
                .whitespace_nowrap()
                .child(StyledText::new(text).with_highlights(highlights))
        });
        div()
            .relative()
            .font_family(style.font_family.clone())
            .text_size(style.font_size)
            .line_height(style.cell.height)
            .text_color(style.foreground)
            .children(lines)
            .children(cursor.map(|(row, col)| {
                let caret = div()
                    .absolute()
                    .left(style.cell.width * col as f32)
                    .top(style.cell.height * row as f32)
                    .w(style.cell.width)
                    .h(style.cell.height);
                if focused {
                    caret.bg(style.cursor.opacity(0.6))
                } else {
                    caret.border_1().border_color(style.cursor.opacity(0.6))
                }
            }))
    }
}

/// One screen row as text plus a highlight per run of same-styled cells.
fn row_runs(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    style: &TerminalStyle,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    // Trailing blank cells with no background only cost shaping time.
    let last = (0..cols)
        .rev()
        .find(|&col| {
            screen.cell(row, col).is_some_and(|cell| {
                cell.has_contents() || cell.bgcolor() != vt100::Color::Default || cell.inverse()
            })
        })
        .map_or(0, |col| col + 1);
    for col in 0..last {
        let Some(cell) = screen.cell(row, col) else { continue };
        if cell.is_wide_continuation() {
            continue;
        }
        let start = text.len();
        if cell.has_contents() {
            text.push_str(cell.contents());
        } else {
            text.push(' ');
        }
        let highlight = cell_highlight(cell, style);
        match highlights.last_mut() {
            Some((range, previous)) if range.end == start && *previous == highlight => {
                range.end = text.len();
            }
            _ => highlights.push((start..text.len(), highlight)),
        }
    }
    highlights.retain(|(_, highlight)| *highlight != HighlightStyle::default());
    (text, highlights)
}

fn cell_highlight(cell: &vt100::Cell, style: &TerminalStyle) -> HighlightStyle {
    let mut foreground = color(cell.fgcolor(), cell.bold());
    let mut background = color(cell.bgcolor(), false);
    if cell.inverse() {
        let (fg, bg) = (foreground.unwrap_or(style.foreground), background.unwrap_or(style.background));
        foreground = Some(bg);
        background = Some(fg);
    }
    HighlightStyle {
        color: foreground.map(|color| if cell.dim() { color.opacity(0.6) } else { color }),
        background_color: background,
        font_weight: cell.bold().then_some(FontWeight::BOLD),
        font_style: cell.italic().then_some(FontStyle::Italic),
        underline: cell.underline().then(|| UnderlineStyle { thickness: px(1.), ..Default::default() }),
        ..Default::default()
    }
}

/// The xterm palette: 16 ANSI colors (bold brightens the first eight), a
/// 6×6×6 cube, then a grayscale ramp.
fn color(color: vt100::Color, bold: bool) -> Option<Hsla> {
    const ANSI: [u32; 16] = [
        0x1e1e1e, 0xe5534b, 0x4fae7d, 0xe0a84a, 0x539bf5, 0xb083f0, 0x39c5cf, 0xd4d4d4,
        0x6e7681, 0xff7b72, 0x7ee2a8, 0xf2cc60, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xffffff,
    ];
    let value = match color {
        vt100::Color::Default => return None,
        vt100::Color::Rgb(r, g, b) => u32::from_be_bytes([0, r, g, b]),
        vt100::Color::Idx(index @ 0..=7) if bold => ANSI[index as usize + 8],
        vt100::Color::Idx(index @ 0..=15) => ANSI[index as usize],
        vt100::Color::Idx(index @ 16..=231) => {
            let index = index - 16;
            let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            u32::from_be_bytes([0, level(index / 36), level(index / 6 % 6), level(index % 6)])
        }
        vt100::Color::Idx(index) => {
            let gray = 8 + (index - 232) * 10;
            u32::from_be_bytes([0, gray, gray, gray])
        }
    };
    Some(rgb(value).into())
}

fn keystroke_input(keystroke: &Keystroke, application_cursor: bool) -> Option<String> {
    let modifiers = &keystroke.modifiers;
    let arrow = |code: char| {
        if application_cursor { format!("\x1bO{code}") } else { format!("\x1b[{code}") }
    };
    let named = match keystroke.key.as_str() {
        "enter" => Some("\r".to_owned()),
        "backspace" if modifiers.control => Some("\x08".to_owned()),
        "backspace" => Some("\x7f".to_owned()),
        "tab" if modifiers.shift => Some("\x1b[Z".to_owned()),
        "tab" => Some("\t".to_owned()),
        "escape" => Some("\x1b".to_owned()),
        "up" => Some(arrow('A')),
        "down" => Some(arrow('B')),
        "right" => Some(arrow('C')),
        "left" => Some(arrow('D')),
        "home" => Some(arrow('H')),
        "end" => Some(arrow('F')),
        "insert" => Some("\x1b[2~".to_owned()),
        "delete" => Some("\x1b[3~".to_owned()),
        "pageup" => Some("\x1b[5~".to_owned()),
        "pagedown" => Some("\x1b[6~".to_owned()),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if modifiers.control && !modifiers.alt {
        let key = keystroke.key.as_bytes();
        return match key {
            [letter @ b'a'..=b'z'] => Some(((letter & 0x1f) as char).to_string()),
            [b'@'] | [b'2'] | [b' '] => Some("\0".to_owned()),
            [b'['] => Some("\x1b".to_owned()),
            [b'\\'] => Some("\x1c".to_owned()),
            [b']'] => Some("\x1d".to_owned()),
            _ => None,
        };
    }
    if modifiers.platform {
        return None;
    }
    let text = keystroke.key_char.clone().or_else(|| {
        (keystroke.key == "space").then(|| " ".to_owned())
    })?;
    Some(if modifiers.alt { format!("\x1b{text}") } else { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn output_redraws_the_line_in_place() {
        let mut screen = TerminalScreen::default();
        screen.process("PS C:\\> gti\x08\x08\x08git status\r\n");
        assert!(screen.contents().starts_with("PS C:\\> git status"));
    }

    #[::core::prelude::v1::test]
    fn keystrokes_become_terminal_input() {
        let input = |text: &str| {
            TerminalScreen::default().input_for(&Keystroke::parse(text).unwrap())
        };
        assert_eq!(input("enter").as_deref(), Some("\r"));
        assert_eq!(input("ctrl-c").as_deref(), Some("\x03"));
        assert_eq!(input("up").as_deref(), Some("\x1b[A"));
        assert_eq!(input("backspace").as_deref(), Some("\x7f"));
    }
}
