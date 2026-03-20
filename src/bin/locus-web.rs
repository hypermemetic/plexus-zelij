#!/usr/bin/env -S cargo +nightly -Zscript
//! Web-based terminal viewer - streams terminal states to browser
//!
//! Usage:
//!   plexus-locus-web [--port 3000]

use anyhow::Result;
use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{
        sse::{Event, Sse},
        Html, Response,
    },
    routing::get,
    Json, Router,
};
use futures::stream::{self, Stream};
use plexus_locus::{
    backend::TerminalBackend,
    backends::tmux::TmuxBackend,
    recording::{engine::PaneRecorder, RecordingSession},
};
use serde::Serialize;
use std::{collections::HashMap, convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::RwLock;

/// Shared app state
#[derive(Clone)]
struct AppState {
    backend: Arc<dyn TerminalBackend>,
    /// Cache of pane contents to avoid excessive queries
    cache: Arc<RwLock<HashMap<String, CachedPane>>>,
}

#[derive(Clone)]
struct CachedPane {
    _id: String,
    _name: Option<String>,
    content: String,
    width: u16,
    height: u16,
    updated_at: std::time::Instant,
}

#[derive(Serialize)]
struct PaneInfo {
    id: String,
    name: Option<String>,
    width: u16,
    height: u16,
    session: String,
}

#[derive(Serialize)]
struct PaneContent {
    id: String,
    content: String,
    html: String,
}

#[derive(Serialize)]
struct LayoutResponse {
    sessions: Vec<SessionLayout>,
}

#[derive(Serialize)]
struct SessionLayout {
    name: String,
    tabs: Vec<TabLayout>,
}

#[derive(Serialize)]
struct TabLayout {
    name: String,
    index: u32,
    panes: Vec<PaneLayout>,
}

#[derive(Serialize)]
struct PaneLayout {
    id: String,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Parse port from args
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.strip_prefix("--port=").map(std::string::ToString::to_string))
        .or_else(|| std::env::args().nth(2))
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    // Initialize backend (tmux for now)
    let backend = Arc::new(TmuxBackend::new()) as Arc<dyn TerminalBackend>;

    if !backend.is_available().await {
        eprintln!("❌ tmux backend not available");
        std::process::exit(1);
    }

    let state = AppState { backend, cache: Arc::new(RwLock::new(HashMap::new())) };

    // Build router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/panes", get(list_panes_handler))
        .route("/api/pane/{id}", get(get_pane_handler))
        .route("/api/layout", get(layout_handler))
        .route("/api/download/pane/{id}", get(download_pane_handler))
        .route(
            "/api/download/tab/{session}/{tab_index}",
            get(download_tab_handler),
        )
        .route("/api/download/session/{session}", get(download_session_handler))
        .route("/api/stream", get(sse_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    tracing::info!("🌐 Locus Web Viewer starting on http://{}", addr);
    tracing::info!("📺 Open your browser to view terminal sessions");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve the HTML frontend
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

/// List all panes
async fn list_panes_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<PaneInfo>>, StatusCode> {
    match state.backend.list_panes(None, None).await {
        Ok(panes) => {
            let info: Vec<PaneInfo> = panes
                .into_iter()
                .map(|p| {
                    // Parse dimensions from pane (tmux provides these)
                    // For now, use defaults
                    PaneInfo {
                        id: p.id.0,
                        name: p.name,
                        width: 80, // TODO: get from backend
                        height: 24,
                        session: p.session.0,
                    }
                })
                .collect();
            Ok(Json(info))
        },
        Err(e) => {
            tracing::error!("Failed to list panes: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        },
    }
}

/// Get specific pane content
async fn get_pane_handler(
    State(state): State<AppState>,
    axum::extract::Path(pane_id): axum::extract::Path<String>,
) -> Result<Json<PaneContent>, StatusCode> {
    // Check cache first
    {
        let cache = state.cache.read().await;
        if let Some(cached) = cache.get(&pane_id) {
            if cached.updated_at.elapsed() < Duration::from_millis(100) {
                return Ok(Json(PaneContent {
                    id: pane_id.clone(),
                    content: cached.content.clone(),
                    html: terminal_to_html(&cached.content, cached.width, cached.height),
                }));
            }
        }
    }

    // Capture screen
    let tmp = format!("/tmp/locus-web-capture-{}", uuid::Uuid::new_v4());
    match state.backend.dump_screen(&tmp, false, Some(&pane_id)).await {
        Ok(content) => {
            let _ = tokio::fs::remove_file(&tmp).await;

            // Update cache
            {
                let mut cache = state.cache.write().await;
                cache.insert(
                    pane_id.clone(),
                    CachedPane {
                        _id: pane_id.clone(),
                        _name: None,
                        content: content.clone(),
                        width: 80,
                        height: 24,
                        updated_at: std::time::Instant::now(),
                    },
                );
            }

            let html = terminal_to_html(&content, 80, 24);
            Ok(Json(PaneContent { id: pane_id, content, html }))
        },
        Err(e) => {
            tracing::error!("Failed to capture pane {}: {}", pane_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        },
    }
}

/// Get tmux layout information (sessions, tabs, panes with positions)
async fn layout_handler(
    State(state): State<AppState>,
) -> Result<Json<LayoutResponse>, StatusCode> {
    // Get all sessions
    let sessions = match state.backend.list_sessions().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to list sessions: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    let mut session_layouts = Vec::new();

    for session in sessions {
        // Get tabs for this session
        let tabs = match state.backend.list_tabs(Some(&session.id.0)).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to list tabs for session {}: {}", session.id.0, e);
                continue;
            },
        };

        let mut tab_layouts = Vec::new();

        for tab in tabs {
            // Get panes for this tab
            let panes = match state.backend.list_panes(Some(&session.id.0), Some(&tab.id.0)).await
            {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Failed to list panes for tab {}: {}", tab.id.0, e);
                    continue;
                },
            };

            let pane_layouts: Vec<PaneLayout> = panes
                .into_iter()
                .map(|p| PaneLayout { id: p.id.0, x: 0, y: 0, width: 80, height: 24 })
                .collect();

            tab_layouts.push(TabLayout {
                name: tab.name.unwrap_or_else(|| format!("{}", tab.index)),
                index: tab.index,
                panes: pane_layouts,
            });
        }

        session_layouts.push(SessionLayout { name: session.name, tabs: tab_layouts });
    }

    Ok(Json(LayoutResponse { sessions: session_layouts }))
}

/// Download a single pane recording as .cast file
async fn download_pane_handler(
    State(_state): State<AppState>,
    axum::extract::Path(pane_id): axum::extract::Path<String>,
) -> Result<Response, StatusCode> {
    tracing::info!("Downloading pane recording: {}", pane_id);

    // Create temporary directory for recording
    let tmp_dir = format!("/tmp/locus-download-{}", uuid::Uuid::new_v4());
    let tmp_path = PathBuf::from(&tmp_dir);
    if let Err(e) = tokio::fs::create_dir_all(&tmp_path).await {
        tracing::error!("Failed to create temp dir: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Start recording the pane
    let recorder = match PaneRecorder::start(pane_id.clone(), &tmp_path, 80, 24, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to start recording for pane {}: {}", pane_id, e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Record for 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Stop recording
    let cast_path = match recorder.stop().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to stop recording for pane {}: {}", pane_id, e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Read the .cast file
    let cast_content = match tokio::fs::read(&cast_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to read .cast file: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Cleanup
    let _ = tokio::fs::remove_dir_all(&tmp_path).await;

    // Return as downloadable file
    let filename = format!("pane-{}.cast", pane_id.replace('%', ""));
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from(cast_content))
        .unwrap())
}

/// Download a tab composite recording as .cast file
async fn download_tab_handler(
    State(_state): State<AppState>,
    axum::extract::Path((session, tab_index)): axum::extract::Path<(String, String)>,
) -> Result<Response, StatusCode> {
    tracing::info!("Downloading tab composite: {}:{}", session, tab_index);

    // Create temporary directory for recording
    let tmp_dir = format!("/tmp/locus-download-{}", uuid::Uuid::new_v4());
    let tmp_path = PathBuf::from(&tmp_dir);
    if let Err(e) = tokio::fs::create_dir_all(&tmp_path).await {
        tracing::error!("Failed to create temp dir: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Start recording session for the tab
    let recording_session = match RecordingSession::start(&session, &tmp_path).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to start recording session: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Record for 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Stop recording
    let cast_files = match recording_session.stop().await {
        Ok(files) => files,
        Err(e) => {
            tracing::error!("Failed to stop recording session: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // If only one pane, return it directly
    if cast_files.len() == 1 {
        let cast_path = &cast_files[0];
        let cast_content = match tokio::fs::read(cast_path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to read .cast file: {}", e);
                let _ = tokio::fs::remove_dir_all(&tmp_path).await;
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            },
        };

        let _ = tokio::fs::remove_dir_all(&tmp_path).await;

        let filename = format!("tab-{}-{}.cast", session, tab_index);
        return Ok(Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            )
            .body(Body::from(cast_content))
            .unwrap());
    }

    // Multiple panes - create composite
    use plexus_locus::compositor::{BorderStyle, CompositeOpts, CompositeWriter};

    let output_path = tmp_path.join("composite.cast");
    let opts = CompositeOpts {
        fps: 10.0,
        idle_time_limit: Some(1.0),
        border_style: BorderStyle::Single,
        title: Some(format!("Tab {}:{}", session, tab_index)),
        theme: None,
    };

    let writer = CompositeWriter::new(&tmp_path, &output_path, opts);
    if let Err(e) = writer.run() {
        tracing::error!("Failed to create composite: {}", e);
        let _ = tokio::fs::remove_dir_all(&tmp_path).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Read the composite
    let cast_content = match tokio::fs::read(&output_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to read composite .cast file: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Cleanup
    let _ = tokio::fs::remove_dir_all(&tmp_path).await;

    // Return as downloadable file
    let filename = format!("tab-{}-{}.cast", session, tab_index);
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from(cast_content))
        .unwrap())
}

/// Download a session composite recording as .cast file (all tabs)
async fn download_session_handler(
    State(_state): State<AppState>,
    axum::extract::Path(session): axum::extract::Path<String>,
) -> Result<Response, StatusCode> {
    tracing::info!("Downloading session composite: {}", session);

    // Create temporary directory for recording
    let tmp_dir = format!("/tmp/locus-download-{}", uuid::Uuid::new_v4());
    let tmp_path = PathBuf::from(&tmp_dir);
    if let Err(e) = tokio::fs::create_dir_all(&tmp_path).await {
        tracing::error!("Failed to create temp dir: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Start recording session
    let recording_session = match RecordingSession::start(&session, &tmp_path).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to start recording session: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Record for 5 seconds
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Stop recording
    let cast_files = match recording_session.stop().await {
        Ok(files) => files,
        Err(e) => {
            tracing::error!("Failed to stop recording session: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // If only one pane, return it directly
    if cast_files.len() == 1 {
        let cast_path = &cast_files[0];
        let cast_content = match tokio::fs::read(cast_path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to read .cast file: {}", e);
                let _ = tokio::fs::remove_dir_all(&tmp_path).await;
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            },
        };

        let _ = tokio::fs::remove_dir_all(&tmp_path).await;

        let filename = format!("session-{}.cast", session);
        return Ok(Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            )
            .body(Body::from(cast_content))
            .unwrap());
    }

    // Multiple panes - create composite
    use plexus_locus::compositor::{BorderStyle, CompositeOpts, CompositeWriter};

    let output_path = tmp_path.join("composite.cast");
    let opts = CompositeOpts {
        fps: 10.0,
        idle_time_limit: Some(1.0),
        border_style: BorderStyle::Single,
        title: Some(format!("Session {}", session)),
        theme: None,
    };

    let writer = CompositeWriter::new(&tmp_path, &output_path, opts);
    if let Err(e) = writer.run() {
        tracing::error!("Failed to create composite: {}", e);
        let _ = tokio::fs::remove_dir_all(&tmp_path).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Read the composite
    let cast_content = match tokio::fs::read(&output_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to read composite .cast file: {}", e);
            let _ = tokio::fs::remove_dir_all(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        },
    };

    // Cleanup
    let _ = tokio::fs::remove_dir_all(&tmp_path).await;

    // Return as downloadable file
    let filename = format!("session-{}.cast", session);
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(Body::from(cast_content))
        .unwrap())
}

/// SSE endpoint for streaming updates
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::unfold(state, |state| async move {
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get all panes
        let panes = match state.backend.list_panes(None, None).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to list panes: {}", e);
                let event = Event::default().data("error");
                return Some((Ok(event), state));
            },
        };

        // Capture each pane
        let mut updates = Vec::new();
        for pane in panes {
            let tmp = format!("/tmp/locus-web-capture-{}", uuid::Uuid::new_v4());
            if let Ok(content) = state.backend.dump_screen(&tmp, false, Some(&pane.id.0)).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                updates.push((pane.id.0.clone(), content));
            }
        }

        // Send as JSON - always succeed
        let event = match Event::default().json_data(&updates) {
            Ok(e) => e,
            Err(_) => Event::default().data("error"),
        };

        Some((Ok(event), state))
    });

    Sse::new(stream)
}

/// Convert terminal text with ANSI codes to styled HTML
fn terminal_to_html(content: &str, _width: u16, height: u16) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"terminal-screen\">");

    for (i, line) in content.lines().enumerate() {
        if i >= height as usize {
            break;
        }
        html.push_str("<div class=\"terminal-line\">");
        html.push_str(&ansi_to_html(line));
        html.push_str("</div>\n");
    }

    html.push_str("</div>");
    html
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace(' ', "&nbsp;")
}

/// Convert ANSI escape sequences to HTML with colors
fn ansi_to_html(line: &str) -> String {
    let mut html = String::new();
    let mut current_fg: Option<&str> = None;
    let mut current_bg: Option<&str> = None;
    let mut bold = false;
    let mut underline = false;
    let mut in_span = false;

    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            // ANSI escape sequence
            chars.next(); // consume '['
            let mut code = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() || c == ';' {
                    code.push(c);
                    chars.next();
                } else if c == 'm' {
                    chars.next(); // consume 'm'
                    break;
                } else {
                    break;
                }
            }

            // Parse SGR codes
            let codes: Vec<u8> = code
                .split(';')
                .filter_map(|s| s.parse().ok())
                .collect();

            for &code in &codes {
                match code {
                    0 => {
                        // Reset
                        if in_span {
                            html.push_str("</span>");
                            in_span = false;
                        }
                        current_fg = None;
                        current_bg = None;
                        bold = false;
                        underline = false;
                    }
                    1 => bold = true,
                    4 => underline = true,
                    22 => bold = false,
                    24 => underline = false,
                    30..=37 => {
                        // Foreground colors
                        current_fg = Some(match code {
                            30 => "#000",
                            31 => "#cd0000",
                            32 => "#00cd00",
                            33 => "#cdcd00",
                            34 => "#0000ee",
                            35 => "#cd00cd",
                            36 => "#00cdcd",
                            37 => "#e5e5e5",
                            _ => unreachable!(),
                        });
                    }
                    40..=47 => {
                        // Background colors
                        current_bg = Some(match code {
                            40 => "#000",
                            41 => "#cd0000",
                            42 => "#00cd00",
                            43 => "#cdcd00",
                            44 => "#0000ee",
                            45 => "#cd00cd",
                            46 => "#00cdcd",
                            47 => "#e5e5e5",
                            _ => unreachable!(),
                        });
                    }
                    90..=97 => {
                        // Bright foreground colors
                        current_fg = Some(match code {
                            90 => "#7f7f7f",
                            91 => "#ff0000",
                            92 => "#00ff00",
                            93 => "#ffff00",
                            94 => "#5c5cff",
                            95 => "#ff00ff",
                            96 => "#00ffff",
                            97 => "#fff",
                            _ => unreachable!(),
                        });
                    }
                    100..=107 => {
                        // Bright background colors
                        current_bg = Some(match code {
                            100 => "#7f7f7f",
                            101 => "#ff0000",
                            102 => "#00ff00",
                            103 => "#ffff00",
                            104 => "#5c5cff",
                            105 => "#ff00ff",
                            106 => "#00ffff",
                            107 => "#fff",
                            _ => unreachable!(),
                        });
                    }
                    _ => {} // Ignore unknown codes
                }
            }

            // Close and reopen span with new styles
            if in_span {
                html.push_str("</span>");
                in_span = false;
            }

            if current_fg.is_some() || current_bg.is_some() || bold || underline {
                html.push_str("<span style=\"");
                if let Some(fg) = current_fg {
                    html.push_str(&format!("color:{};", fg));
                }
                if let Some(bg) = current_bg {
                    html.push_str(&format!("background-color:{};", bg));
                }
                if bold {
                    html.push_str("font-weight:bold;");
                }
                if underline {
                    html.push_str("text-decoration:underline;");
                }
                html.push_str("\">");
                in_span = true;
            }
        } else {
            // Regular character
            html.push_str(&html_escape(&ch.to_string()));
        }
    }

    if in_span {
        html.push_str("</span>");
    }

    html
}
