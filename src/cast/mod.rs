//! Asciicast v2 format types and I/O.
//!
//! This module provides types and functionality for reading and writing
//! asciicast v2 format files (.cast). The format is NDJSON (newline-delimited JSON)
//! with a header line followed by event lines.
//!
//! See: <https://docs.asciinema.org/manual/asciicast/v2>/

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CastError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid cast file: missing header")]
    MissingHeader,

    #[error("Invalid cast file: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, CastError>;

/// Asciicast v2 header.
///
/// This is the first line of a .cast file, containing metadata about the recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CastHeader {
    /// Format version (always 2 for v2).
    pub version: u8,

    /// Terminal width in columns.
    pub width: u16,

    /// Terminal height in rows.
    pub height: u16,

    /// Unix timestamp (seconds since epoch) when recording started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,

    /// Environment variables (typically SHELL and TERM).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Recording title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Idle time limit in seconds. If set, long pauses are truncated to this duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_time_limit: Option<f64>,

    /// Theme configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
}

/// Color theme for rendering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Theme {
    /// Foreground color (hex format: "#RRGGBB").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,

    /// Background color (hex format: "#RRGGBB").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,

    /// 16-color palette (ANSI colors 0-15).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
}

/// Asciicast v2 event.
///
/// Events are written as JSON arrays: [time, `event_type`, ...data]
#[derive(Debug, Clone, PartialEq)]
pub enum CastEvent {
    /// Output event: [time, "o", data]
    Output(f64, String),

    /// Input event: [time, "i", data]
    Input(f64, String),

    /// Resize event: [time, "r", cols, rows]
    Resize(f64, u16, u16),

    /// Marker event: [time, "m", label]
    Marker(f64, String),
}

impl CastEvent {
    /// Get the timestamp of this event.
    pub const fn time(&self) -> f64 {
        match self {
            Self::Output(t, _) => *t,
            Self::Input(t, _) => *t,
            Self::Resize(t, _, _) => *t,
            Self::Marker(t, _) => *t,
        }
    }
}

impl Serialize for CastEvent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        match self {
            Self::Output(time, data) => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element(time)?;
                seq.serialize_element("o")?;
                seq.serialize_element(data)?;
                seq.end()
            },
            Self::Input(time, data) => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element(time)?;
                seq.serialize_element("i")?;
                seq.serialize_element(data)?;
                seq.end()
            },
            Self::Resize(time, cols, rows) => {
                let mut seq = serializer.serialize_seq(Some(4))?;
                seq.serialize_element(time)?;
                seq.serialize_element("r")?;
                seq.serialize_element(&format!("{cols}x{rows}"))?;
                seq.end()
            },
            Self::Marker(time, label) => {
                let mut seq = serializer.serialize_seq(Some(3))?;
                seq.serialize_element(time)?;
                seq.serialize_element("m")?;
                seq.serialize_element(label)?;
                seq.end()
            },
        }
    }
}

impl<'de> Deserialize<'de> for CastEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let arr = serde_json::Value::deserialize(deserializer)?;
        let arr = arr.as_array().ok_or_else(|| D::Error::custom("expected array"))?;

        if arr.len() < 2 {
            return Err(D::Error::custom("event array too short"));
        }

        let time = arr[0].as_f64().ok_or_else(|| D::Error::custom("invalid time"))?;
        let event_type = arr[1].as_str().ok_or_else(|| D::Error::custom("invalid event type"))?;

        match event_type {
            "o" => {
                if arr.len() < 3 {
                    return Err(D::Error::custom("output event missing data"));
                }
                let data = arr[2]
                    .as_str()
                    .ok_or_else(|| D::Error::custom("invalid output data"))?
                    .to_string();
                Ok(Self::Output(time, data))
            },
            "i" => {
                if arr.len() < 3 {
                    return Err(D::Error::custom("input event missing data"));
                }
                let data = arr[2]
                    .as_str()
                    .ok_or_else(|| D::Error::custom("invalid input data"))?
                    .to_string();
                Ok(Self::Input(time, data))
            },
            "r" => {
                if arr.len() < 3 {
                    return Err(D::Error::custom("resize event missing dimensions"));
                }
                let dims =
                    arr[2].as_str().ok_or_else(|| D::Error::custom("invalid resize dimensions"))?;
                let parts: Vec<&str> = dims.split('x').collect();
                if parts.len() != 2 {
                    return Err(D::Error::custom("invalid resize format"));
                }
                let cols: u16 = parts[0].parse().map_err(|_| D::Error::custom("invalid cols"))?;
                let rows: u16 = parts[1].parse().map_err(|_| D::Error::custom("invalid rows"))?;
                Ok(Self::Resize(time, cols, rows))
            },
            "m" => {
                if arr.len() < 3 {
                    return Err(D::Error::custom("marker event missing label"));
                }
                let label = arr[2]
                    .as_str()
                    .ok_or_else(|| D::Error::custom("invalid marker label"))?
                    .to_string();
                Ok(Self::Marker(time, label))
            },
            _ => Err(D::Error::custom(format!("unknown event type: {event_type}"))),
        }
    }
}

/// Append-only writer for .cast files.
///
/// Writes asciicast v2 format (NDJSON): header line followed by event lines.
pub struct CastWriter {
    writer: BufWriter<File>,
}

impl CastWriter {
    /// Create a new writer for the given file path.
    ///
    /// The file is created (or truncated if it exists).
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::create(path)?;
        Ok(Self { writer: BufWriter::new(file) })
    }

    /// Write the header line.
    ///
    /// This must be called exactly once before writing any events.
    pub fn write_header(&mut self, header: &CastHeader) -> Result<()> {
        let json = serde_json::to_string(header)?;
        writeln!(self.writer, "{json}")?;
        Ok(())
    }

    /// Write an event line.
    pub fn write_event(&mut self, event: &CastEvent) -> Result<()> {
        let json = serde_json::to_string(event)?;
        writeln!(self.writer, "{json}")?;
        Ok(())
    }

    /// Flush buffered data to disk.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    /// Flush and close the file.
    ///
    /// This consumes the writer and ensures all data is written to disk.
    pub fn finish(mut self) -> Result<()> {
        self.flush()?;
        Ok(())
    }
}

/// Streaming reader for .cast files.
///
/// Reads asciicast v2 format (NDJSON): header line followed by event lines.
pub struct CastReader {
    reader: BufReader<File>,
    header: CastHeader,
}

impl CastReader {
    /// Open a .cast file for reading.
    ///
    /// The header is read immediately and can be accessed via `header()`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Read header line
        let mut header_line = String::new();
        reader.read_line(&mut header_line)?;

        if header_line.is_empty() {
            return Err(CastError::MissingHeader);
        }

        let header: CastHeader = serde_json::from_str(&header_line)?;

        if header.version != 2 {
            return Err(CastError::InvalidFormat(format!(
                "unsupported version: {}",
                header.version
            )));
        }

        Ok(Self { reader, header })
    }

    /// Get the header.
    pub const fn header(&self) -> &CastHeader {
        &self.header
    }

    /// Get an iterator over events.
    ///
    /// Each line after the header is parsed as a `CastEvent`.
    pub fn events(self) -> EventIterator {
        EventIterator { reader: self.reader }
    }
}

/// Iterator over events in a .cast file.
pub struct EventIterator {
    reader: BufReader<File>,
}

impl Iterator for EventIterator {
    type Item = Result<CastEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();

        match self.reader.read_line(&mut line) {
            Ok(0) => None, // EOF
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    // Skip empty lines
                    return self.next();
                }
                Some(serde_json::from_str(line).map_err(CastError::from))
            },
            Err(e) => Some(Err(CastError::from(e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_header_serialization() {
        let mut env = HashMap::new();
        env.insert("SHELL".to_string(), "/bin/bash".to_string());
        env.insert("TERM".to_string(), "xterm-256color".to_string());

        let header = CastHeader {
            version: 2,
            width: 80,
            height: 24,
            timestamp: Some(1234567890),
            env: Some(env),
            title: Some("Test Recording".to_string()),
            idle_time_limit: Some(2.0),
            theme: Some(Theme {
                fg: Some("#ffffff".to_string()),
                bg: Some("#000000".to_string()),
                palette: None,
            }),
        };

        let json = serde_json::to_string(&header).unwrap();
        let parsed: CastHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_minimal_header() {
        let header = CastHeader {
            version: 2,
            width: 80,
            height: 24,
            timestamp: None,
            env: None,
            title: None,
            idle_time_limit: None,
            theme: None,
        };

        let json = serde_json::to_string(&header).unwrap();
        let parsed: CastHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_event_serialization() {
        let events = vec![
            CastEvent::Output(0.0, "Hello, World!\n".to_string()),
            CastEvent::Input(1.5, "ls\n".to_string()),
            CastEvent::Resize(2.0, 120, 40),
            CastEvent::Marker(3.0, "checkpoint".to_string()),
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let parsed: CastEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event, parsed);
        }
    }

    #[test]
    fn test_event_time() {
        assert_eq!(CastEvent::Output(1.5, "test".to_string()).time(), 1.5);
        assert_eq!(CastEvent::Input(2.5, "test".to_string()).time(), 2.5);
        assert_eq!(CastEvent::Resize(3.5, 80, 24).time(), 3.5);
        assert_eq!(CastEvent::Marker(4.5, "test".to_string()).time(), 4.5);
    }

    #[test]
    fn test_unicode_output() {
        let event = CastEvent::Output(1.0, "Hello 世界 🌍\n".to_string());
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CastEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_escape_sequences() {
        let event = CastEvent::Output(1.0, "\x1b[31mRed\x1b[0m\n".to_string());
        let json = serde_json::to_string(&event).unwrap();

        // JSON should escape control characters
        assert!(json.contains("\\u001b"));

        let parsed: CastEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_empty_output() {
        let event = CastEvent::Output(1.0, String::new());
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CastEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn test_roundtrip_write_read() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_roundtrip.cast");

        // Write
        {
            let mut writer = CastWriter::create(&path).unwrap();

            let header = CastHeader {
                version: 2,
                width: 80,
                height: 24,
                timestamp: Some(1234567890),
                env: None,
                title: Some("Roundtrip Test".to_string()),
                idle_time_limit: None,
                theme: None,
            };

            writer.write_header(&header).unwrap();
            writer.write_event(&CastEvent::Output(0.0, "Hello\n".to_string())).unwrap();
            writer.write_event(&CastEvent::Output(1.0, "World\n".to_string())).unwrap();
            writer.write_event(&CastEvent::Resize(2.0, 120, 40)).unwrap();
            writer.write_event(&CastEvent::Marker(3.0, "test".to_string())).unwrap();
            writer.finish().unwrap();
        }

        // Read
        {
            let reader = CastReader::open(&path).unwrap();

            assert_eq!(reader.header().version, 2);
            assert_eq!(reader.header().width, 80);
            assert_eq!(reader.header().height, 24);
            assert_eq!(reader.header().title, Some("Roundtrip Test".to_string()));

            let events: Vec<CastEvent> = reader.events().collect::<Result<Vec<_>>>().unwrap();

            assert_eq!(events.len(), 4);
            assert_eq!(events[0], CastEvent::Output(0.0, "Hello\n".to_string()));
            assert_eq!(events[1], CastEvent::Output(1.0, "World\n".to_string()));
            assert_eq!(events[2], CastEvent::Resize(2.0, 120, 40));
            assert_eq!(events[3], CastEvent::Marker(3.0, "test".to_string()));
        }

        // Cleanup
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_roundtrip_unicode_and_escapes() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_unicode_escapes.cast");

        // Write
        {
            let mut writer = CastWriter::create(&path).unwrap();

            let header = CastHeader {
                version: 2,
                width: 80,
                height: 24,
                timestamp: None,
                env: None,
                title: None,
                idle_time_limit: None,
                theme: None,
            };

            writer.write_header(&header).unwrap();
            writer
                .write_event(&CastEvent::Output(0.0, "Unicode: 世界 🌍\n".to_string()))
                .unwrap();
            writer
                .write_event(&CastEvent::Output(1.0, "\x1b[31mRed\x1b[0m\n".to_string()))
                .unwrap();
            writer
                .write_event(&CastEvent::Output(2.0, "Null:\x00 Bell:\x07\n".to_string()))
                .unwrap();
            writer.finish().unwrap();
        }

        // Read
        {
            let reader = CastReader::open(&path).unwrap();
            let events: Vec<CastEvent> = reader.events().collect::<Result<Vec<_>>>().unwrap();

            assert_eq!(events.len(), 3);
            assert_eq!(events[0], CastEvent::Output(0.0, "Unicode: 世界 🌍\n".to_string()));
            assert_eq!(events[1], CastEvent::Output(1.0, "\x1b[31mRed\x1b[0m\n".to_string()));
            assert_eq!(events[2], CastEvent::Output(2.0, "Null:\x00 Bell:\x07\n".to_string()));
        }

        // Cleanup
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_empty_file_error() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_empty.cast");

        // Create empty file
        File::create(&path).unwrap();

        let result = CastReader::open(&path);
        assert!(matches!(result, Err(CastError::MissingHeader)));

        // Cleanup
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_invalid_version() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_invalid_version.cast");

        // Write header with wrong version
        {
            let mut file = File::create(&path).unwrap();
            writeln!(file, r#"{{"version":1,"width":80,"height":24}}"#).unwrap();
        }

        let result = CastReader::open(&path);
        assert!(matches!(result, Err(CastError::InvalidFormat(_))));

        // Cleanup
        std::fs::remove_file(&path).ok();
    }
}
