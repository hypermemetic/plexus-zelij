//! Terminal state management using vt100 emulation
//!
//! Maintains in-memory terminal state for each pane, enabling:
//! - Instant queries (no file I/O)
//! - Rich terminal queries (cursor, regions, attributes)
//! - Event-driven updates
//! - Incremental fetching

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, watch};
use vt100::Parser;

/// Terminal state for a single pane
pub struct PaneTerminalState {
    /// Pane identifier (e.g., "%0")
    pub pane_id: String,
    /// vt100 parser maintaining terminal state
    parser: Parser,
    /// Last time this pane was updated
    last_update: Instant,
    /// Monotonic sequence number (increments on each update)
    update_sequence: u64,
    /// Channels for notifying watchers of updates
    change_notifiers: Vec<watch::Sender<u64>>,
}

impl PaneTerminalState {
    /// Create new terminal state for a pane
    pub fn new(pane_id: String, width: u16, height: u16) -> Self {
        Self {
            pane_id,
            parser: Parser::new(height, width, 0),
            last_update: Instant::now(),
            update_sequence: 0,
            change_notifiers: Vec::new(),
        }
    }

    /// Process new output data (incremental)
    pub fn process(&mut self, data: &[u8]) {
        self.parser.process(data);
        self.last_update = Instant::now();
        self.update_sequence += 1;

        // Notify all watchers
        self.notify_watchers();
    }

    /// Get current screen contents as plain text
    pub fn contents(&self) -> String {
        self.parser.screen().contents()
    }

    /// Get screen contents with ANSI formatting preserved
    pub fn contents_formatted(&self) -> Vec<u8> {
        self.parser.screen().contents_formatted()
    }

    /// Get cursor position (row, col) - 0-indexed
    pub fn cursor_position(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// Get specific region of terminal (rows start..end)
    pub fn region(&self, start_row: u16, end_row: u16) -> Vec<u8> {
        self.parser
            .screen()
            .rows_formatted(start_row, end_row)
            .flatten()
            .collect()
    }

    /// Get terminal dimensions
    pub fn dimensions(&self) -> (u16, u16) {
        let screen = self.parser.screen();
        (screen.size().1, screen.size().0) // (width, height)
    }

    /// Get terminal title
    pub fn title(&self) -> String {
        self.parser.screen().title().to_string()
    }

    /// Check if in alternate screen mode
    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Get last update time
    pub fn last_update(&self) -> Instant {
        self.last_update
    }

    /// Get current sequence number
    pub fn sequence(&self) -> u64 {
        self.update_sequence
    }

    /// Resize terminal (when pane dimensions change)
    pub fn resize(&mut self, width: u16, height: u16) {
        let new_parser = Parser::new(height, width, 0);
        // Copy existing content to new size
        let content = self.parser.screen().contents();
        let mut resized = new_parser;
        resized.process(content.as_bytes());
        self.parser = resized;
        self.update_sequence += 1;
        self.notify_watchers();
    }

    /// Subscribe to changes (returns receiver that gets notified on updates)
    pub fn subscribe(&mut self) -> watch::Receiver<u64> {
        let (tx, rx) = watch::channel(self.update_sequence);
        self.change_notifiers.push(tx);
        rx
    }

    /// Notify all watchers of update
    fn notify_watchers(&self) {
        for notifier in &self.change_notifiers {
            let _ = notifier.send(self.update_sequence);
        }
    }
}

/// Global manager for all pane terminal states
pub struct TerminalStateManager {
    states: Arc<RwLock<HashMap<String, Arc<RwLock<PaneTerminalState>>>>>,
}

impl TerminalStateManager {
    /// Create new terminal state manager
    pub fn new() -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start tracking a pane
    pub async fn track_pane(&self, pane_id: &str, width: u16, height: u16) -> Result<()> {
        let state = PaneTerminalState::new(pane_id.to_string(), width, height);
        self.states
            .write()
            .await
            .insert(pane_id.to_string(), Arc::new(RwLock::new(state)));
        Ok(())
    }

    /// Stop tracking a pane (cleanup)
    pub async fn untrack_pane(&self, pane_id: &str) -> Result<()> {
        self.states.write().await.remove(pane_id);
        Ok(())
    }

    /// Feed incremental data to a pane's terminal state
    pub async fn process_output(&self, pane_id: &str, data: &[u8]) -> Result<()> {
        if let Some(state) = self.states.read().await.get(pane_id) {
            let mut s = state.write().await;
            s.process(data);
        }
        Ok(())
    }

    /// Get current screen contents
    pub async fn get_contents(&self, pane_id: &str) -> Option<String> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.contents())
    }

    /// Get screen contents with ANSI formatting
    pub async fn get_contents_formatted(&self, pane_id: &str) -> Option<Vec<u8>> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.contents_formatted())
    }

    /// Get cursor position
    pub async fn get_cursor(&self, pane_id: &str) -> Option<(u16, u16)> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.cursor_position())
    }

    /// Get specific region of terminal
    pub async fn get_region(&self, pane_id: &str, start_row: u16, end_row: u16) -> Option<Vec<u8>> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.region(start_row, end_row))
    }

    /// Get terminal dimensions
    pub async fn get_dimensions(&self, pane_id: &str) -> Option<(u16, u16)> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.dimensions())
    }

    /// Get terminal title
    pub async fn get_title(&self, pane_id: &str) -> Option<String> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.title())
    }

    /// Check if pane is in alternate screen
    pub async fn is_alternate_screen(&self, pane_id: &str) -> Option<bool> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.alternate_screen())
    }

    /// Get current sequence number for a pane
    pub async fn get_sequence(&self, pane_id: &str) -> Option<u64> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?;
        let s = state.read().await;
        Some(s.sequence())
    }

    /// Subscribe to changes for a pane
    pub async fn subscribe(&self, pane_id: &str) -> Option<watch::Receiver<u64>> {
        let states = self.states.read().await;
        let state = states.get(pane_id)?.clone();
        drop(states); // Release read lock before acquiring write lock
        let mut s = state.write().await;
        Some(s.subscribe())
    }

    /// Resize a pane's terminal
    pub async fn resize_pane(&self, pane_id: &str, width: u16, height: u16) -> Result<()> {
        if let Some(state) = self.states.read().await.get(pane_id) {
            let mut s = state.write().await;
            s.resize(width, height);
        }
        Ok(())
    }

    /// Check if a pane is being tracked
    pub async fn is_tracked(&self, pane_id: &str) -> bool {
        self.states.read().await.contains_key(pane_id)
    }

    /// Get list of all tracked panes
    pub async fn tracked_panes(&self) -> Vec<String> {
        self.states.read().await.keys().cloned().collect()
    }

    /// Get info about all tracked panes
    pub async fn get_all_info(&self) -> Vec<PaneStateInfo> {
        let states = self.states.read().await;
        let mut infos = Vec::new();

        for (pane_id, state) in states.iter() {
            let s = state.read().await;
            let (width, height) = s.dimensions();
            infos.push(PaneStateInfo {
                pane_id: pane_id.clone(),
                width,
                height,
                sequence: s.sequence(),
            });
        }

        infos
    }
}

impl Default for TerminalStateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a pane's state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct PaneStateInfo {
    pub pane_id: String,
    pub width: u16,
    pub height: u16,
    pub sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_terminal_state_basic() {
        let manager = TerminalStateManager::new();

        // Track a pane
        manager.track_pane("%0", 80, 24).await.unwrap();

        // Feed some output
        manager.process_output("%0", b"Hello, world!\n").await.unwrap();

        // Query content
        let content = manager.get_contents("%0").await.unwrap();
        assert!(content.contains("Hello, world!"));

        // Check sequence incremented
        let seq = manager.get_sequence("%0").await.unwrap();
        assert_eq!(seq, 1);
    }

    #[tokio::test]
    async fn test_cursor_position() {
        let manager = TerminalStateManager::new();
        manager.track_pane("%1", 80, 24).await.unwrap();

        // Move cursor with ANSI escape
        manager
            .process_output("%1", b"\x1b[5;10H")
            .await
            .unwrap();

        let (row, col) = manager.get_cursor("%1").await.unwrap();
        assert_eq!(row, 4); // 0-indexed
        assert_eq!(col, 9);
    }

    #[tokio::test]
    async fn test_resize() {
        let manager = TerminalStateManager::new();
        manager.track_pane("%2", 80, 24).await.unwrap();

        // Write some content
        manager
            .process_output("%2", b"Test content")
            .await
            .unwrap();

        // Resize
        manager.resize_pane("%2", 120, 30).await.unwrap();

        let (width, height) = manager.get_dimensions("%2").await.unwrap();
        assert_eq!(width, 120);
        assert_eq!(height, 30);
    }

    #[tokio::test]
    async fn test_subscribe() {
        let manager = TerminalStateManager::new();
        manager.track_pane("%3", 80, 24).await.unwrap();

        let mut rx = manager.subscribe("%3").await.unwrap();

        // Send update
        manager.process_output("%3", b"update").await.unwrap();

        // Should receive notification
        assert!(rx.changed().await.is_ok());
        assert_eq!(*rx.borrow(), 1);
    }

    #[tokio::test]
    async fn test_untrack() {
        let manager = TerminalStateManager::new();
        manager.track_pane("%4", 80, 24).await.unwrap();

        assert!(manager.is_tracked("%4").await);

        manager.untrack_pane("%4").await.unwrap();

        assert!(!manager.is_tracked("%4").await);
    }
}
