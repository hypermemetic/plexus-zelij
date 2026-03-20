#!/usr/bin/env -S cargo +nightly -Zscript
//! Web-based terminal viewer - streams terminal states to browser
//!
//! Usage:
//!   plexus-locus-web [--port 3000]

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, sse::{Event, Sse}},
    routing::get,
    Json, Router,
};
use futures::stream::{self, Stream};
use serde::Serialize;
use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use plexus_locus::{backend::TerminalBackend, backends::tmux::TmuxBackend};

/// Shared app state
#[derive(Clone)]
struct AppState {
    backend: Arc<dyn TerminalBackend>,
    /// Cache of pane contents to avoid excessive queries
    cache: Arc<RwLock<HashMap<String, CachedPane>>>,
}

#[derive(Clone)]
struct CachedPane {
    id: String,
    name: Option<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    // Parse port from args
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.strip_prefix("--port=").map(|s| s.to_string()))
        .or_else(|| std::env::args().nth(2))
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    // Initialize backend (tmux for now)
    let backend = Arc::new(TmuxBackend::new()) as Arc<dyn TerminalBackend>;

    if !backend.is_available().await {
        eprintln!("❌ tmux backend not available");
        std::process::exit(1);
    }

    let state = AppState {
        backend,
        cache: Arc::new(RwLock::new(HashMap::new())),
    };

    // Build router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/panes", get(list_panes_handler))
        .route("/api/pane/{id}", get(get_pane_handler))
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
                        width: 80,  // TODO: get from backend
                        height: 24,
                        session: p.session.0,
                    }
                })
                .collect();
            Ok(Json(info))
        }
        Err(e) => {
            tracing::error!("Failed to list panes: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
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
                        id: pane_id.clone(),
                        name: None,
                        content: content.clone(),
                        width: 80,
                        height: 24,
                        updated_at: std::time::Instant::now(),
                    },
                );
            }

            let html = terminal_to_html(&content, 80, 24);
            Ok(Json(PaneContent {
                id: pane_id,
                content,
                html,
            }))
        }
        Err(e) => {
            tracing::error!("Failed to capture pane {}: {}", pane_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
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
            }
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

/// Convert terminal text to styled HTML
fn terminal_to_html(content: &str, _width: u16, height: u16) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"terminal-screen\">");

    for (i, line) in content.lines().enumerate() {
        if i >= height as usize {
            break;
        }
        html.push_str("<div class=\"terminal-line\">");
        html.push_str(&html_escape(line));
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
