//! Frame diffing and ANSI escape sequence generation.
//!
//! This module provides efficient diffing between consecutive frames to minimize
//! the amount of data written to output files.

use super::frame::{Cell, Color, CompositeFrame};

/// State tracking for SGR attributes during rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StyleState {
    fg: Color,
    bg: Color,
    bold: bool,
    underline: bool,
    inverse: bool,
    italic: bool,
}

impl Default for StyleState {
    fn default() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            underline: false,
            inverse: false,
            italic: false,
        }
    }
}

impl StyleState {
    /// Generate SGR escape sequence to transition from current state to target cell.
    ///
    /// Returns empty string if no change needed.
    fn transition_to(&mut self, cell: &Cell) -> String {
        let mut sgr_params = Vec::new();

        // Check if we need to reset
        let needs_reset = (!cell.bold && self.bold)
            || (!cell.underline && self.underline)
            || (!cell.inverse && self.inverse)
            || (!cell.italic && self.italic);

        if needs_reset {
            sgr_params.push("0".to_string());
            *self = StyleState::default();
        }

        // Set attributes
        if cell.bold && !self.bold {
            sgr_params.push("1".to_string());
            self.bold = true;
        }

        if cell.italic && !self.italic {
            sgr_params.push("3".to_string());
            self.italic = true;
        }

        if cell.underline && !self.underline {
            sgr_params.push("4".to_string());
            self.underline = true;
        }

        if cell.inverse && !self.inverse {
            sgr_params.push("7".to_string());
            self.inverse = true;
        }

        // Set colors
        if cell.fg != self.fg {
            sgr_params.push(cell.fg.to_ansi_fg());
            self.fg = cell.fg;
        }

        if cell.bg != self.bg {
            sgr_params.push(cell.bg.to_ansi_bg());
            self.bg = cell.bg;
        }

        if sgr_params.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", sgr_params.join(";"))
        }
    }
}

/// Render a complete frame from scratch.
///
/// This is used for the first frame or after a resize event.
/// Emits clear screen, cursor home, and all cell contents.
pub fn render_full_frame(frame: &CompositeFrame) -> String {
    let mut output = String::new();

    // Clear screen and move cursor to home
    output.push_str("\x1b[2J\x1b[H");

    let mut style = StyleState::default();

    for (row_idx, row) in frame.cells.iter().enumerate() {
        // Position cursor at start of row
        output.push_str(&format!("\x1b[{};1H", row_idx + 1));

        for cell in row {
            // Emit style changes if needed
            let sgr = style.transition_to(cell);
            if !sgr.is_empty() {
                output.push_str(&sgr);
            }

            // Emit character
            output.push(cell.ch);
        }
    }

    // Reset at end
    output.push_str("\x1b[0m");

    output
}

/// Diff two consecutive frames and emit minimal ANSI escape sequences.
///
/// Compares cells and only emits changes. Coalesces adjacent changed cells
/// into single writes to minimize cursor movement commands.
///
/// If dimensions changed, returns a full re-render via `render_full_frame`.
pub fn diff_frames(prev: &CompositeFrame, next: &CompositeFrame) -> String {
    // If dimensions changed, do full re-render
    if prev.width != next.width || prev.height != next.height {
        return render_full_frame(next);
    }

    // If frames are identical, return empty string
    if frames_equal(prev, next) {
        return String::new();
    }

    let mut output = String::new();
    let mut style = StyleState::default();

    for row_idx in 0..next.height as usize {
        // Find runs of changed cells in this row
        let mut col_idx = 0;

        while col_idx < next.width as usize {
            let prev_cell = &prev.cells[row_idx][col_idx];
            let next_cell = &next.cells[row_idx][col_idx];

            if prev_cell == next_cell {
                // No change, skip to next cell
                col_idx += 1;
                continue;
            }

            // Found a change - find the run of consecutive changes
            let run_start = col_idx;
            let mut run_end = col_idx + 1;

            while run_end < next.width as usize {
                let prev_cell = &prev.cells[row_idx][run_end];
                let next_cell = &next.cells[row_idx][run_end];

                if prev_cell == next_cell {
                    break;
                }

                run_end += 1;
            }

            // Emit cursor position for start of run
            // Note: ANSI positions are 1-indexed
            output.push_str(&format!("\x1b[{};{}H", row_idx + 1, run_start + 1));

            // Emit all cells in the run
            for col in run_start..run_end {
                let cell = &next.cells[row_idx][col];

                // Emit style changes if needed
                let sgr = style.transition_to(cell);
                if !sgr.is_empty() {
                    output.push_str(&sgr);
                }

                // Emit character
                output.push(cell.ch);
            }

            col_idx = run_end;
        }
    }

    // Reset at end if we emitted anything
    if !output.is_empty() {
        output.push_str("\x1b[0m");
    }

    output
}

/// Check if two frames are identical.
fn frames_equal(a: &CompositeFrame, b: &CompositeFrame) -> bool {
    if a.width != b.width || a.height != b.height {
        return false;
    }

    for row_idx in 0..a.height as usize {
        for col_idx in 0..a.width as usize {
            if a.cells[row_idx][col_idx] != b.cells[row_idx][col_idx] {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_full_frame_empty() {
        let frame = CompositeFrame::new(5, 3);
        let output = render_full_frame(&frame);

        // Should contain clear screen and home
        assert!(output.contains("\x1b[2J"));
        assert!(output.contains("\x1b[H"));

        // Should contain reset at end
        assert!(output.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_render_full_frame_with_content() {
        let mut frame = CompositeFrame::new(3, 2);
        frame.set_cell(0, 0, Cell::new('A'));
        frame.set_cell(0, 1, Cell::new('B'));
        frame.set_cell(0, 2, Cell::new('C'));
        frame.set_cell(1, 0, Cell::new('D'));
        frame.set_cell(1, 1, Cell::new('E'));
        frame.set_cell(1, 2, Cell::new('F'));

        let output = render_full_frame(&frame);

        // Should contain all characters
        assert!(output.contains('A'));
        assert!(output.contains('B'));
        assert!(output.contains('C'));
        assert!(output.contains('D'));
        assert!(output.contains('E'));
        assert!(output.contains('F'));

        // Should contain cursor positioning
        assert!(output.contains("\x1b[1;1H")); // First row
        assert!(output.contains("\x1b[2;1H")); // Second row
    }

    #[test]
    fn test_diff_frames_identical() {
        let frame1 = CompositeFrame::new(5, 3);
        let frame2 = CompositeFrame::new(5, 3);

        let diff = diff_frames(&frame1, &frame2);

        // Identical frames should produce empty diff
        assert_eq!(diff, "");
    }

    #[test]
    fn test_diff_frames_dimension_change() {
        let frame1 = CompositeFrame::new(5, 3);
        let frame2 = CompositeFrame::new(6, 3);

        let diff = diff_frames(&frame1, &frame2);

        // Dimension change should trigger full re-render
        assert!(diff.contains("\x1b[2J"));
        assert!(diff.contains("\x1b[H"));
    }

    #[test]
    fn test_diff_frames_single_cell_change() {
        let mut frame1 = CompositeFrame::new(5, 3);
        frame1.set_cell(1, 2, Cell::new(' '));

        let mut frame2 = CompositeFrame::new(5, 3);
        frame2.set_cell(1, 2, Cell::new('X'));

        let diff = diff_frames(&frame1, &frame2);

        // Should contain cursor move to position (1-indexed)
        assert!(diff.contains("\x1b[2;3H"));

        // Should contain the changed character
        assert!(diff.contains('X'));

        // Should contain reset at end
        assert!(diff.ends_with("\x1b[0m"));
    }

    #[test]
    fn test_diff_frames_full_row_change() {
        let mut frame1 = CompositeFrame::new(5, 3);
        // Leave row 1 as all spaces

        let mut frame2 = CompositeFrame::new(5, 3);
        frame2.set_cell(1, 0, Cell::new('H'));
        frame2.set_cell(1, 1, Cell::new('E'));
        frame2.set_cell(1, 2, Cell::new('L'));
        frame2.set_cell(1, 3, Cell::new('L'));
        frame2.set_cell(1, 4, Cell::new('O'));

        let diff = diff_frames(&frame1, &frame2);

        // Should position cursor at start of row 2 (1-indexed)
        assert!(diff.contains("\x1b[2;1H"));

        // Should contain all changed characters
        assert!(diff.contains('H'));
        assert!(diff.contains('E'));
        assert!(diff.contains('L'));
        assert!(diff.contains('O'));
    }

    #[test]
    fn test_diff_frames_color_change() {
        let mut frame1 = CompositeFrame::new(3, 1);
        let mut cell1 = Cell::new('A');
        cell1.fg = Color::Default;
        frame1.set_cell(0, 0, cell1);

        let mut frame2 = CompositeFrame::new(3, 1);
        let mut cell2 = Cell::new('A');
        cell2.fg = Color::Indexed(1); // Red
        frame2.set_cell(0, 0, cell2);

        let diff = diff_frames(&frame1, &frame2);

        // Should contain cursor positioning
        assert!(diff.contains("\x1b[1;1H"));

        // Should contain color change (red foreground)
        assert!(diff.contains("31"));

        // Should contain the character (even though it's the same)
        assert!(diff.contains('A'));
    }

    #[test]
    fn test_diff_frames_attribute_change() {
        let mut frame1 = CompositeFrame::new(3, 1);
        let mut cell1 = Cell::new('B');
        cell1.bold = false;
        frame1.set_cell(0, 0, cell1);

        let mut frame2 = CompositeFrame::new(3, 1);
        let mut cell2 = Cell::new('B');
        cell2.bold = true;
        frame2.set_cell(0, 0, cell2);

        let diff = diff_frames(&frame1, &frame2);

        // Should contain bold attribute
        assert!(diff.contains("\x1b[1m") || diff.contains(";1;") || diff.contains(";1m"));
    }

    #[test]
    fn test_diff_frames_multiple_attributes() {
        let mut frame1 = CompositeFrame::new(3, 1);
        frame1.set_cell(0, 0, Cell::new('T'));

        let mut frame2 = CompositeFrame::new(3, 1);
        let mut cell = Cell::new('T');
        cell.bold = true;
        cell.underline = true;
        cell.fg = Color::Indexed(2); // Green
        frame2.set_cell(0, 0, cell);

        let diff = diff_frames(&frame1, &frame2);

        // Should contain bold (1)
        assert!(diff.contains('1'));

        // Should contain underline (4)
        assert!(diff.contains('4'));

        // Should contain green foreground (32)
        assert!(diff.contains("32"));
    }

    #[test]
    fn test_diff_frames_coalescing() {
        let frame1 = CompositeFrame::new(10, 1);
        // All spaces

        let mut frame2 = CompositeFrame::new(10, 1);
        // Change three consecutive cells
        frame2.set_cell(0, 3, Cell::new('A'));
        frame2.set_cell(0, 4, Cell::new('B'));
        frame2.set_cell(0, 5, Cell::new('C'));

        let diff = diff_frames(&frame1, &frame2);

        // Should only have ONE cursor position command for the run
        let cursor_count = diff.matches("\x1b[1;").count();
        assert_eq!(cursor_count, 1, "Should only have one cursor position for consecutive changes");

        // Should position at column 4 (1-indexed)
        assert!(diff.contains("\x1b[1;4H"));

        // Should contain all three characters
        assert!(diff.contains('A'));
        assert!(diff.contains('B'));
        assert!(diff.contains('C'));
    }

    #[test]
    fn test_diff_frames_gap_in_changes() {
        let frame1 = CompositeFrame::new(10, 1);
        // All spaces

        let mut frame2 = CompositeFrame::new(10, 1);
        // Change with gap in middle
        frame2.set_cell(0, 1, Cell::new('X'));
        frame2.set_cell(0, 2, Cell::new('Y'));
        frame2.set_cell(0, 5, Cell::new('Z'));

        let diff = diff_frames(&frame1, &frame2);

        // Should have TWO cursor positions (one for each run)
        let cursor_count = diff.matches("\x1b[1;").count();
        assert_eq!(cursor_count, 2, "Should have two cursor positions for non-consecutive changes");

        // Should contain both positions
        assert!(diff.contains("\x1b[1;2H")); // For X,Y run
        assert!(diff.contains("\x1b[1;6H")); // For Z
    }

    #[test]
    fn test_diff_frames_rgb_colors() {
        let mut frame1 = CompositeFrame::new(2, 1);
        frame1.set_cell(0, 0, Cell::new('R'));

        let mut frame2 = CompositeFrame::new(2, 1);
        let mut cell = Cell::new('R');
        cell.fg = Color::Rgb(255, 128, 64);
        frame2.set_cell(0, 0, cell);

        let diff = diff_frames(&frame1, &frame2);

        // Should contain RGB color code
        assert!(diff.contains("38;2;255;128;64"));
    }

    #[test]
    fn test_diff_frames_background_color() {
        let mut frame1 = CompositeFrame::new(2, 1);
        frame1.set_cell(0, 0, Cell::new('G'));

        let mut frame2 = CompositeFrame::new(2, 1);
        let mut cell = Cell::new('G');
        cell.bg = Color::Indexed(4); // Blue background
        frame2.set_cell(0, 0, cell);

        let diff = diff_frames(&frame1, &frame2);

        // Should contain blue background (44)
        assert!(diff.contains("44"));
    }

    #[test]
    fn test_diff_frames_inverse_attribute() {
        let mut frame1 = CompositeFrame::new(2, 1);
        frame1.set_cell(0, 0, Cell::new('I'));

        let mut frame2 = CompositeFrame::new(2, 1);
        let mut cell = Cell::new('I');
        cell.inverse = true;
        frame2.set_cell(0, 0, cell);

        let diff = diff_frames(&frame1, &frame2);

        // Should contain inverse attribute (7)
        assert!(diff.contains('7'));
    }

    #[test]
    fn test_diff_frames_reset_on_attribute_removal() {
        let mut frame1 = CompositeFrame::new(3, 1);
        let mut cell1 = Cell::new('R');
        cell1.bold = true;
        frame1.set_cell(0, 0, cell1);
        frame1.set_cell(0, 1, Cell::new('X')); // Unchanged
        frame1.set_cell(0, 2, Cell::new('Y')); // Unchanged

        let mut frame2 = CompositeFrame::new(3, 1);
        let cell2 = Cell::new('R'); // Bold removed
        frame2.set_cell(0, 0, cell2);
        frame2.set_cell(0, 1, Cell::new('X')); // Unchanged
        frame2.set_cell(0, 2, Cell::new('Y')); // Unchanged

        let diff = diff_frames(&frame1, &frame2);

        // Should contain reset code (0) to remove bold
        assert!(diff.contains("\x1b[0"));
    }

    #[test]
    fn test_style_state_transition() {
        let mut state = StyleState::default();

        let mut cell = Cell::new('T');
        cell.bold = true;
        cell.fg = Color::Indexed(1);

        let sgr = state.transition_to(&cell);

        // Should set bold and red color
        assert!(sgr.contains('1'));
        assert!(sgr.contains("31"));

        // State should be updated
        assert_eq!(state.bold, true);
        assert_eq!(state.fg, Color::Indexed(1));

        // Transitioning to same state should produce no output
        let sgr2 = state.transition_to(&cell);
        assert_eq!(sgr2, "");
    }

    #[test]
    fn test_frames_equal() {
        let mut frame1 = CompositeFrame::new(3, 2);
        frame1.set_cell(0, 0, Cell::new('A'));

        let mut frame2 = CompositeFrame::new(3, 2);
        frame2.set_cell(0, 0, Cell::new('A'));

        assert!(frames_equal(&frame1, &frame2));

        // Change one cell
        frame2.set_cell(0, 1, Cell::new('B'));
        assert!(!frames_equal(&frame1, &frame2));
    }

    #[test]
    fn test_frames_equal_different_dimensions() {
        let frame1 = CompositeFrame::new(3, 2);
        let frame2 = CompositeFrame::new(3, 3);

        assert!(!frames_equal(&frame1, &frame2));
    }
}
