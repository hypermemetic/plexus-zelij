use clap::{Parser, Subcommand};
use plexus_locus::{Locus, TmuxBackend, Zellij};
use plexus_locus::compositor::{BorderStyle, CompositeOpts, CompositeWriter};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use std::sync::Arc;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "locus")]
#[command(about = "Locus — terminal workspace orchestration over Plexus RPC")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Run in stdio mode (MCP-compatible)
    #[arg(long)]
    stdio: bool,

    /// Port for WebSocket server
    #[arg(short, long, default_value = "44480")]
    port: u16,

    /// Terminal backend: auto, tmux, zellij
    #[arg(long, default_value = "auto")]
    backend: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Render a recording to a composite .cast file (standalone mode)
    Render {
        /// Recording directory containing pane-*.cast and layout.jsonl
        recording_dir: PathBuf,

        /// Output .cast file path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Frame rate in frames per second
        #[arg(long, default_value = "30.0")]
        fps: f64,

        /// Maximum idle time between events in seconds
        #[arg(long, default_value = "2.0")]
        idle_limit: f64,

        /// Border style: single, double, heavy, none
        #[arg(long, default_value = "single")]
        border: String,

        /// Preview mode: render a single frame at timestamp
        #[arg(long)]
        preview: bool,

        /// Timestamp for preview mode (in seconds)
        #[arg(long, default_value = "0.0")]
        time: f64,
    },
    /// Start RPC server (default)
    Serve {
        /// Run in stdio mode (MCP-compatible)
        #[arg(long)]
        stdio: bool,

        /// Port for WebSocket server
        #[arg(short, long, default_value = "4448")]
        port: u16,

        /// Terminal backend: auto, tmux, zellij
        #[arg(long, default_value = "auto")]
        backend: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Handle standalone render command
    match args.command {
        Some(Command::Render {
            recording_dir,
            output,
            fps,
            idle_limit,
            border,
            preview,
            time,
        }) => {
            return run_standalone_render(recording_dir, output, fps, idle_limit, border, preview, time);
        }
        Some(Command::Serve { stdio, port, backend }) => {
            return run_server(stdio, port, backend).await;
        }
        None => {
            // Default to server mode with args from top level
            return run_server(args.stdio, args.port, args.backend).await;
        }
    }
}

fn run_standalone_render(
    recording_dir: PathBuf,
    output: Option<PathBuf>,
    fps: f64,
    idle_limit: f64,
    border: String,
    preview: bool,
    time: f64,
) -> anyhow::Result<()> {
    // Validate recording directory
    if !recording_dir.exists() || !recording_dir.is_dir() {
        anyhow::bail!("Recording directory not found: {}", recording_dir.display());
    }

    if preview {
        // Preview mode - render single frame
        use plexus_locus::compositor::Compositor;

        eprintln!("Rendering preview at t={:.2}s...", time);

        let mut compositor = Compositor::new(&recording_dir)?;
        compositor.build_timeline()?;

        let frame = compositor.render_frame(time)?;
        let content = frame.render_ansi();

        // Print preview to stdout
        println!("{}", content);

        eprintln!("\nPreview rendered: {}x{} at t={:.2}s", frame.width, frame.height, time);
    } else {
        // Full render mode
        let output_path = output.unwrap_or_else(|| recording_dir.join("composite.cast"));

        eprintln!("Rendering recording: {}", recording_dir.display());
        eprintln!("Output: {}", output_path.display());
        eprintln!("Options: fps={}, idle_limit={}s, border={}", fps, idle_limit, border);

        let border_style = match border.to_lowercase().as_str() {
            "double" => BorderStyle::Double,
            "heavy" => BorderStyle::Heavy,
            "none" => BorderStyle::None,
            _ => BorderStyle::Single,
        };

        let opts = CompositeOpts {
            fps,
            idle_time_limit: Some(idle_limit),
            border_style,
            title: None,
            theme: None,
        };

        let writer = CompositeWriter::new(&recording_dir, &output_path, opts)
            .with_progress(|progress| {
                eprint!("\rProgress: {:.1}%", progress * 100.0);
                if progress >= 1.0 {
                    eprintln!(); // New line on completion
                }
            });

        let result = writer.run()?;

        eprintln!("\nRender complete!");
        eprintln!("  Output: {}", result.output_path.display());
        eprintln!("  Duration: {:.2}s", result.duration_secs);
        eprintln!("  Frames: {}", result.frame_count);
        eprintln!("  Size: {} bytes", result.total_bytes);
    }

    Ok(())
}

async fn run_server(stdio: bool, port: u16, backend: String) -> anyhow::Result<()> {
    let filter = if stdio {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("plexus_locus=warn"))
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,plexus_locus=debug"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Build sub-activations with selected backend
    let locus = match backend.as_str() {
        "tmux" => {
            tracing::info!("Using tmux backend");
            Locus::new(TmuxBackend::new())
        }
        "zellij" => {
            tracing::info!("Using zellij backend");
            Locus::new(Zellij::new())
        }
        _ => {
            // Auto-detect: $TMUX → tmux, $ZELLIJ_SESSION_NAME → zellij, else tmux
            if std::env::var("TMUX").is_ok() {
                tracing::info!("Auto-detected tmux backend");
                Locus::new(TmuxBackend::new())
            } else if std::env::var("ZELLIJ_SESSION_NAME").is_ok() {
                tracing::info!("Auto-detected zellij backend");
                Locus::new(Zellij::new())
            } else {
                tracing::info!("No multiplexer detected, defaulting to tmux backend");
                Locus::new(TmuxBackend::new())
            }
        }
    };

    // Register sub-activations flat on the DynamicHub for clean routing:
    //   synapse locus sessions list
    //   synapse locus panes capture --pane %5
    //   synapse locus info status
    //   synapse locus recording start
    //   synapse locus render render
    let hub = Arc::new(
        DynamicHub::new("locus")
            .register(locus.sessions)
            .register(locus.tabs)
            .register(locus.panes)
            .register(locus.workspace)
            .register(locus.info)
            .register(locus.recording)
            .register(locus.render)
    );

    let rpc_converter = |arc| {
        DynamicHub::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))
    };

    let mut builder = TransportServer::builder(hub, rpc_converter);

    if stdio {
        builder = builder.with_stdio();
        tracing::info!("Starting Locus in stdio mode");
    } else {
        builder = builder.with_websocket(port);
        tracing::info!("Locus listening on ws://127.0.0.1:{}", port);
    }

    builder.build().await?.serve().await
}
