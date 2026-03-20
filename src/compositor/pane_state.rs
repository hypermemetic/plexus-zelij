//! Per-pane state management with vt100 parser.

use crate::compositor::frame::Cell;

/// State for a single pane, maintaining a vt100 parser.
///
/// This struct holds the terminal emulator state for one pane, processing
/// raw output bytes and handling resize events.
pub struct PaneState {
    /// Pane identifier (e.g., "%5").
    pub pane_id: String,

    /// Current width in columns.
    pub width: u16,

    /// Current height in rows.
    pub height: u16,

    /// VT100 parser for this pane.
    parser: vt100::Parser,
}

impl PaneState {
    /// Create a new pane state with the given dimensions.
    ///
    /// # Arguments
    /// * `pane_id` - Unique identifier for this pane
    /// * `width` - Initial width in columns
    /// * `height` - Initial height in rows
    pub fn new(pane_id: String, width: u16, height: u16) -> Self {
        // Create parser with zero scrollback (we don't need it for compositing)
        let parser = vt100::Parser::new(height, width, 0);

        Self { pane_id, width, height, parser }
    }

    /// Process raw output bytes.
    ///
    /// This feeds the bytes into the vt100 parser, updating the terminal state.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Resize the pane.
    ///
    /// This creates a new parser with the new dimensions. The old terminal
    /// state is lost, which matches how terminals typically handle resizes.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        // Create a new parser with new dimensions
        self.parser = vt100::Parser::new(height, width, 0);
    }

    /// Get the screen representation.
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Read a cell at the given position.
    ///
    /// Returns None if the position is out of bounds.
    ///
    /// # Arguments
    /// * `row` - Row index (0-based)
    /// * `col` - Column index (0-based)
    pub fn cell(&self, row: u16, col: u16) -> Option<Cell> {
        if row >= self.height || col >= self.width {
            return None;
        }

        let vt_cell = self.parser.screen().cell(row, col)?;
        Some(Cell::from_vt100(vt_cell))
    }

    /// Get all cells as a 2D vector.
    ///
    /// Returns cells[row][col] for the entire pane.
    pub fn cells(&self) -> Vec<Vec<Cell>> {
        let mut cells = Vec::with_capacity(self.height as usize);

        for row in 0..self.height {
            let mut row_cells = Vec::with_capacity(self.width as usize);
            for col in 0..self.width {
                if let Some(vt_cell) = self.parser.screen().cell(row, col) {
                    row_cells.push(Cell::from_vt100(vt_cell));
                } else {
                    row_cells.push(Cell::default());
                }
            }
            cells.push(row_cells);
        }

        cells
    }

    /// Get the cursor position.
    ///
    /// Returns (row, col) in 0-based coordinates.
    pub fn cursor_position(&self) -> (u16, u16) {
        let screen = self.parser.screen();
        (screen.cursor_position().0, screen.cursor_position().1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_state_creation() {
        let state = PaneState::new("%5".to_string(), 80, 24);
        assert_eq!(state.pane_id, "%5");
        assert_eq!(state.width, 80);
        assert_eq!(state.height, 24);
    }

    #[test]
    fn test_pane_state_process_simple() {
        let mut state = PaneState::new("%1".to_string(), 10, 5);

        // Write "Hello"
        state.process(b"Hello");

        // Check that we can read it back
        let cell = state.cell(0, 0).unwrap();
        assert_eq!(cell.ch, 'H');

        let cell = state.cell(0, 1).unwrap();
        assert_eq!(cell.ch, 'e');

        let cell = state.cell(0, 2).unwrap();
        assert_eq!(cell.ch, 'l');
    }

    #[test]
    fn test_pane_state_process_with_ansi() {
        let mut state = PaneState::new("%1".to_string(), 20, 5);

        // Write colored text
        state.process(b"\x1b[31mRed\x1b[0m");

        // First character should be 'R'
        let cell = state.cell(0, 0).unwrap();
        assert_eq!(cell.ch, 'R');
    }

    #[test]
    fn test_pane_state_resize() {
        let mut state = PaneState::new("%1".to_string(), 10, 5);

        state.process(b"Hello");

        // Resize (this clears the state in our implementation)
        state.resize(20, 10);

        assert_eq!(state.width, 20);
        assert_eq!(state.height, 10);

        // Old content is lost after resize
        let cell = state.cell(0, 0).unwrap();
        assert_eq!(cell.ch, ' '); // Should be blank
    }

    #[test]
    fn test_pane_state_out_of_bounds() {
        let state = PaneState::new("%1".to_string(), 10, 5);

        assert!(state.cell(100, 100).is_none());
        assert!(state.cell(10, 0).is_none());
        assert!(state.cell(0, 10).is_none());
    }

    #[test]
    fn test_pane_state_cells() {
        let mut state = PaneState::new("%1".to_string(), 3, 2);

        state.process(b"ABC\r\nDEF");

        let cells = state.cells();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].len(), 3);

        assert_eq!(cells[0][0].ch, 'A');
        assert_eq!(cells[0][1].ch, 'B');
        assert_eq!(cells[0][2].ch, 'C');
        assert_eq!(cells[1][0].ch, 'D');
        assert_eq!(cells[1][1].ch, 'E');
        assert_eq!(cells[1][2].ch, 'F');
    }

    #[test]
    fn test_pane_state_cursor_position() {
        let mut state = PaneState::new("%1".to_string(), 10, 5);

        state.process(b"Hello");

        // Cursor should be at column 5 (after "Hello")
        let (row, col) = state.cursor_position();
        assert_eq!(row, 0);
        assert_eq!(col, 5);
    }

    #[test]
    fn test_pane_state_newlines() {
        let mut state = PaneState::new("%1".to_string(), 10, 5);

        state.process(b"Line1\r\nLine2\r\nLine3");

        let cell = state.cell(0, 0).unwrap();
        assert_eq!(cell.ch, 'L');

        let cell = state.cell(1, 0).unwrap();
        assert_eq!(cell.ch, 'L');

        let cell = state.cell(2, 0).unwrap();
        assert_eq!(cell.ch, 'L');
    }
}
