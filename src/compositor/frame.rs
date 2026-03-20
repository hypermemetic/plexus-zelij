//! Composite frame representation and ANSI rendering.

use std::fmt;

/// Color representation for terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Default terminal color (foreground or background).
    Default,

    /// Indexed color (0-255 in the xterm 256-color palette).
    Indexed(u8),

    /// RGB color.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Convert from vt100 color type.
    pub fn from_vt100(color: vt100::Color) -> Self {
        match color {
            vt100::Color::Default => Color::Default,
            vt100::Color::Idx(idx) => Color::Indexed(idx),
            vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        }
    }

    /// Convert to ANSI escape sequence component.
    ///
    /// Returns the parameter string for SGR sequences (without the CSI prefix/suffix).
    pub fn to_ansi_fg(&self) -> String {
        match self {
            Color::Default => "39".to_string(),
            Color::Indexed(idx) if *idx < 8 => format!("{}", 30 + idx),
            Color::Indexed(idx) if *idx < 16 => format!("{}", 82 + idx),
            Color::Indexed(idx) => format!("38;5;{}", idx),
            Color::Rgb(r, g, b) => format!("38;2;{};{};{}", r, g, b),
        }
    }

    pub fn to_ansi_bg(&self) -> String {
        match self {
            Color::Default => "49".to_string(),
            Color::Indexed(idx) if *idx < 8 => format!("{}", 40 + idx),
            Color::Indexed(idx) if *idx < 16 => format!("{}", 92 + idx),
            Color::Indexed(idx) => format!("48;5;{}", idx),
            Color::Rgb(r, g, b) => format!("48;2;{};{};{}", r, g, b),
        }
    }
}

/// A single terminal cell with character and styling information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The character displayed in this cell (space if empty).
    pub ch: char,

    /// Foreground color.
    pub fg: Color,

    /// Background color.
    pub bg: Color,

    /// Bold attribute.
    pub bold: bool,

    /// Underline attribute.
    pub underline: bool,

    /// Inverse/reverse video attribute.
    pub inverse: bool,

    /// Italic attribute.
    pub italic: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            underline: false,
            inverse: false,
            italic: false,
        }
    }
}

impl Cell {
    /// Create a new cell with default styling.
    pub fn new(ch: char) -> Self {
        Self {
            ch,
            ..Default::default()
        }
    }

    /// Create a cell from vt100 cell data.
    pub fn from_vt100(cell: &vt100::Cell) -> Self {
        Self {
            ch: cell.contents().chars().next().unwrap_or(' '),
            fg: Color::from_vt100(cell.fgcolor()),
            bg: Color::from_vt100(cell.bgcolor()),
            bold: cell.bold(),
            underline: cell.underline(),
            inverse: cell.inverse(),
            italic: cell.italic(),
        }
    }
}

/// A complete composite frame representing the entire terminal screen.
///
/// This contains all pane contents positioned according to the layout,
/// with borders drawn between panes.
#[derive(Debug, Clone)]
pub struct CompositeFrame {
    /// Total width in columns.
    pub width: u16,

    /// Total height in rows.
    pub height: u16,

    /// 2D grid of cells: cells[row][col]
    pub cells: Vec<Vec<Cell>>,
}

impl CompositeFrame {
    /// Create a new blank frame with the given dimensions.
    pub fn new(width: u16, height: u16) -> Self {
        let cells = vec![vec![Cell::default(); width as usize]; height as usize];
        Self {
            width,
            height,
            cells,
        }
    }

    /// Set a cell at the given position.
    ///
    /// Does nothing if the position is out of bounds.
    pub fn set_cell(&mut self, row: u16, col: u16, cell: Cell) {
        if row < self.height && col < self.width {
            self.cells[row as usize][col as usize] = cell;
        }
    }

    /// Get a cell at the given position.
    ///
    /// Returns None if out of bounds.
    pub fn get_cell(&self, row: u16, col: u16) -> Option<&Cell> {
        if row < self.height && col < self.width {
            Some(&self.cells[row as usize][col as usize])
        } else {
            None
        }
    }

    /// Render the frame as ANSI escape sequences.
    ///
    /// This produces a string that can be written to a terminal or saved to a file.
    /// The output includes:
    /// - Cursor positioning (absolute, not relative)
    /// - SGR (Select Graphic Rendition) sequences for colors and attributes
    /// - Optimized to minimize redundant escape sequences
    pub fn render_ansi(&self) -> String {
        let mut output = String::new();

        // Start with clear screen and home cursor
        output.push_str("\x1b[2J\x1b[H");

        let mut current_fg = Color::Default;
        let mut current_bg = Color::Default;
        let mut current_bold = false;
        let mut current_underline = false;
        let mut current_inverse = false;
        let mut current_italic = false;

        for (row_idx, row) in self.cells.iter().enumerate() {
            // Position cursor at start of row
            output.push_str(&format!("\x1b[{};1H", row_idx + 1));

            for cell in row {
                // Build SGR sequence if attributes changed
                let mut sgr_params = Vec::new();

                // Check if we need to reset
                let needs_reset =
                    (!cell.bold && current_bold) ||
                    (!cell.underline && current_underline) ||
                    (!cell.inverse && current_inverse) ||
                    (!cell.italic && current_italic);

                if needs_reset {
                    sgr_params.push("0".to_string());
                    current_fg = Color::Default;
                    current_bg = Color::Default;
                    current_bold = false;
                    current_underline = false;
                    current_inverse = false;
                    current_italic = false;
                }

                // Set attributes
                if cell.bold && !current_bold {
                    sgr_params.push("1".to_string());
                    current_bold = true;
                }

                if cell.underline && !current_underline {
                    sgr_params.push("4".to_string());
                    current_underline = true;
                }

                if cell.inverse && !current_inverse {
                    sgr_params.push("7".to_string());
                    current_inverse = true;
                }

                if cell.italic && !current_italic {
                    sgr_params.push("3".to_string());
                    current_italic = true;
                }

                // Set colors
                if cell.fg != current_fg {
                    sgr_params.push(cell.fg.to_ansi_fg());
                    current_fg = cell.fg;
                }

                if cell.bg != current_bg {
                    sgr_params.push(cell.bg.to_ansi_bg());
                    current_bg = cell.bg;
                }

                // Emit SGR sequence if needed
                if !sgr_params.is_empty() {
                    output.push_str("\x1b[");
                    output.push_str(&sgr_params.join(";"));
                    output.push('m');
                }

                // Emit character
                output.push(cell.ch);
            }
        }

        // Reset at end
        output.push_str("\x1b[0m");

        output
    }
}

impl fmt::Display for CompositeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render_ansi())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_conversions() {
        assert_eq!(Color::Default.to_ansi_fg(), "39");
        assert_eq!(Color::Default.to_ansi_bg(), "49");
        assert_eq!(Color::Indexed(1).to_ansi_fg(), "31");
        assert_eq!(Color::Indexed(9).to_ansi_fg(), "91");
        assert_eq!(Color::Indexed(100).to_ansi_fg(), "38;5;100");
        assert_eq!(Color::Rgb(255, 128, 64).to_ansi_fg(), "38;2;255;128;64");
    }

    #[test]
    fn test_cell_default() {
        let cell = Cell::default();
        assert_eq!(cell.ch, ' ');
        assert_eq!(cell.fg, Color::Default);
        assert_eq!(cell.bg, Color::Default);
        assert!(!cell.bold);
        assert!(!cell.underline);
        assert!(!cell.inverse);
    }

    #[test]
    fn test_frame_creation() {
        let frame = CompositeFrame::new(80, 24);
        assert_eq!(frame.width, 80);
        assert_eq!(frame.height, 24);
        assert_eq!(frame.cells.len(), 24);
        assert_eq!(frame.cells[0].len(), 80);
    }

    #[test]
    fn test_frame_set_get_cell() {
        let mut frame = CompositeFrame::new(10, 10);
        let cell = Cell::new('X');

        frame.set_cell(5, 5, cell.clone());
        assert_eq!(frame.get_cell(5, 5).unwrap().ch, 'X');

        // Out of bounds
        frame.set_cell(100, 100, cell.clone());
        assert!(frame.get_cell(100, 100).is_none());
    }

    #[test]
    fn test_frame_render_simple() {
        let mut frame = CompositeFrame::new(3, 2);
        frame.set_cell(0, 0, Cell::new('A'));
        frame.set_cell(0, 1, Cell::new('B'));
        frame.set_cell(0, 2, Cell::new('C'));
        frame.set_cell(1, 0, Cell::new('D'));
        frame.set_cell(1, 1, Cell::new('E'));
        frame.set_cell(1, 2, Cell::new('F'));

        let ansi = frame.render_ansi();

        // Should contain clear screen
        assert!(ansi.contains("\x1b[2J"));

        // Should contain the characters
        assert!(ansi.contains('A'));
        assert!(ansi.contains('B'));
        assert!(ansi.contains('C'));
        assert!(ansi.contains('D'));
        assert!(ansi.contains('E'));
        assert!(ansi.contains('F'));
    }

    #[test]
    fn test_frame_render_with_colors() {
        let mut frame = CompositeFrame::new(2, 1);

        let mut cell1 = Cell::new('R');
        cell1.fg = Color::Indexed(1); // Red

        let mut cell2 = Cell::new('G');
        cell2.fg = Color::Indexed(2); // Green

        frame.set_cell(0, 0, cell1);
        frame.set_cell(0, 1, cell2);

        let ansi = frame.render_ansi();

        // Should contain color codes
        assert!(ansi.contains("31")); // Red foreground
        assert!(ansi.contains("32")); // Green foreground
    }
}
