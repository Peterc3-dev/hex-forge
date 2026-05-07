use std::io;
use std::path::Path;

use crate::editor::Editor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Hex,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    Hex,
    Ascii,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    GotoOffset,
    Search,
    QuitConfirm,
}

pub struct App {
    pub editor: Editor,
    pub cursor: usize,
    pub scroll_offset: usize, // in lines (each line = 16 bytes)
    pub visible_lines: usize,
    pub view: View,
    pub edit_mode: EditMode,
    pub input_mode: InputMode,
    pub input_buf: String,
    pub modified: bool,
    pub selection_start: Option<usize>,
    pub clipboard: Option<String>,
    pub status_msg: Option<String>,
    pub search_pattern: Vec<u8>,
    pub search_query: String,
    /// For hex input: accumulates the high nibble before committing a byte
    pub hex_nibble: Option<u8>,
    pub info_scroll: usize,
}

impl App {
    pub fn open(path: &Path, readonly: bool) -> io::Result<Self> {
        let editor = Editor::open(path, readonly)?;
        Ok(Self {
            editor,
            cursor: 0,
            scroll_offset: 0,
            visible_lines: 20,
            view: View::Hex,
            edit_mode: EditMode::Hex,
            input_mode: InputMode::Normal,
            input_buf: String::new(),
            modified: false,
            selection_start: None,
            clipboard: None,
            status_msg: None,
            search_pattern: Vec::new(),
            search_query: String::new(),
            hex_nibble: None,
            info_scroll: 0,
        })
    }

    pub fn file_size(&self) -> usize {
        self.editor.file_size()
    }

    pub fn filename(&self) -> String {
        self.editor
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "???".into())
    }

    // -- Navigation --

    pub fn move_cursor_up(&mut self) {
        if self.cursor >= 16 {
            self.cursor -= 16;
        }
        self.ensure_cursor_visible();
    }

    pub fn move_cursor_down(&mut self) {
        if self.cursor + 16 < self.file_size() {
            self.cursor += 16;
        } else {
            self.cursor = self.file_size().saturating_sub(1);
        }
        self.ensure_cursor_visible();
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.ensure_cursor_visible();
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor + 1 < self.file_size() {
            self.cursor += 1;
        }
        self.ensure_cursor_visible();
    }

    pub fn page_up(&mut self) {
        let page = self.visible_lines.saturating_sub(1) * 16;
        self.cursor = self.cursor.saturating_sub(page);
        self.scroll_offset = self.scroll_offset.saturating_sub(self.visible_lines.saturating_sub(1));
        self.ensure_cursor_visible();
    }

    pub fn page_down(&mut self) {
        let page = self.visible_lines.saturating_sub(1) * 16;
        self.cursor = (self.cursor + page).min(self.file_size().saturating_sub(1));
        self.ensure_cursor_visible();
    }

    pub fn ensure_cursor_visible(&mut self) {
        let cursor_line = self.cursor / 16;
        if cursor_line < self.scroll_offset {
            self.scroll_offset = cursor_line;
        }
        if cursor_line >= self.scroll_offset + self.visible_lines {
            self.scroll_offset = cursor_line - self.visible_lines + 1;
        }
    }

    // -- Editing --

    pub fn input_hex_digit(&mut self, c: char) {
        if self.editor.readonly {
            self.status_msg = Some("Read-only mode".into());
            return;
        }

        let nibble = match c.to_ascii_lowercase() {
            '0'..='9' => c as u8 - b'0',
            'a'..='f' => c as u8 - b'a' + 10,
            _ => return,
        };

        if let Some(high) = self.hex_nibble.take() {
            let byte = (high << 4) | nibble;
            self.editor.write_byte(self.cursor, byte);
            self.modified = true;
            // Advance cursor
            if self.cursor + 1 < self.file_size() {
                self.cursor += 1;
            }
            self.ensure_cursor_visible();
        } else {
            self.hex_nibble = Some(nibble);
        }
    }

    pub fn input_ascii_char(&mut self, c: char) {
        if self.editor.readonly {
            self.status_msg = Some("Read-only mode".into());
            return;
        }

        self.editor.write_byte(self.cursor, c as u8);
        self.modified = true;
        if self.cursor + 1 < self.file_size() {
            self.cursor += 1;
        }
        self.ensure_cursor_visible();
    }

    // -- Save --

    pub fn save(&mut self) -> io::Result<()> {
        match self.editor.save() {
            Ok(()) => {
                self.modified = false;
                self.status_msg = Some("Saved".to_string());
                Ok(())
            }
            Err(e) => {
                self.status_msg = Some(format!("Save failed: {}", e));
                Err(e)
            }
        }
    }

    // -- Goto --

    pub fn execute_goto(&mut self) {
        let input = self.input_buf.trim().to_string();
        let offset = if let Some(hex) = input.strip_prefix("0x").or_else(|| input.strip_prefix("0X")) {
            usize::from_str_radix(hex, 16).ok()
        } else {
            input.parse::<usize>().ok()
        };

        match offset {
            Some(off) if off < self.file_size() => {
                self.cursor = off;
                self.ensure_cursor_visible();
                self.status_msg = Some(format!("Jumped to 0x{:X}", off));
            }
            Some(off) => {
                self.status_msg = Some(format!("Offset 0x{:X} is beyond file size", off));
            }
            None => {
                self.status_msg = Some("Invalid offset".into());
            }
        }
        self.input_buf.clear();
    }

    // -- Search --

    pub fn execute_search(&mut self) {
        let input = self.input_buf.trim().to_string();
        self.input_buf.clear();

        if input.is_empty() {
            self.status_msg = Some("Empty search".into());
            return;
        }

        // Try to parse as hex pattern (contains only hex digits and spaces)
        let hex_input = input.replace(' ', "");
        let is_hex = hex_input.len() % 2 == 0
            && hex_input.chars().all(|c| c.is_ascii_hexdigit());

        let pattern: Vec<u8> = if is_hex && hex_input.len() >= 2 {
            // Parse hex
            (0..hex_input.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&hex_input[i..i + 2], 16).ok())
                .collect()
        } else {
            // ASCII
            input.as_bytes().to_vec()
        };

        if pattern.is_empty() {
            self.status_msg = Some("Invalid search pattern".into());
            return;
        }

        self.search_query = input;
        self.search_pattern = pattern.clone();

        match self.editor.search(&pattern, self.cursor) {
            Some(pos) => {
                self.cursor = pos;
                self.ensure_cursor_visible();
                self.status_msg = Some(format!("Found at 0x{:X}", pos));
            }
            None => {
                // Wrap around
                match self.editor.search(&pattern, 0) {
                    Some(pos) => {
                        self.cursor = pos;
                        self.ensure_cursor_visible();
                        self.status_msg = Some(format!("Found at 0x{:X} (wrapped)", pos));
                    }
                    None => {
                        self.status_msg = Some("Pattern not found".into());
                    }
                }
            }
        }
    }

    pub fn find_next(&mut self) {
        if self.search_pattern.is_empty() {
            self.status_msg = Some("No previous search".into());
            return;
        }
        let start = self.cursor + 1;
        match self.editor.search(&self.search_pattern, start) {
            Some(pos) => {
                self.cursor = pos;
                self.ensure_cursor_visible();
                self.status_msg = Some(format!("Found at 0x{:X}", pos));
            }
            None => {
                // Wrap around
                match self.editor.search(&self.search_pattern, 0) {
                    Some(pos) if pos <= self.cursor => {
                        self.cursor = pos;
                        self.ensure_cursor_visible();
                        self.status_msg = Some(format!("Found at 0x{:X} (wrapped)", pos));
                    }
                    _ => {
                        self.status_msg = Some("No more matches".into());
                    }
                }
            }
        }
    }

    // -- Selection / Copy --

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_start.map(|start| {
            let lo = start.min(self.cursor);
            let hi = start.max(self.cursor);
            (lo, hi)
        })
    }

    pub fn copy_selection(&mut self) {
        if let Some((lo, hi)) = self.selection_range() {
            let bytes = self.editor.read_range(lo, hi - lo + 1);
            let hex: Vec<String> = bytes.iter().map(|b| format!("{:02X}", b)).collect();
            let hex_str = hex.join(" ");
            self.clipboard = Some(hex_str.clone());
            self.selection_start = None;
            let count = hi - lo + 1;
            self.status_msg = Some(format!("Copied {} bytes to clipboard", count));
        }
    }
}
