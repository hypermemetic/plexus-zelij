use clap::Parser;
use plexus_locus::{Locus, TmuxBackend, Zellij};
use plexus_core::plexus::DynamicHub;
use plexus_transport::TransportServer;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "locus")]
#[command(about = "Locus — terminal workspace orchestration over Plexus RPC")]
struct Args {
    /// Run in stdio mode (MCP-compatible)
    #[arg(long)]
    stdio: bool,

    /// Port for WebSocket server
    #[arg(short, long, default_value = "4448")]
    port: u16,

    /// Terminal backend: auto, tmux, zellij
    #[arg(long, default_value = "auto")]
    backend: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let filter = if args.stdio {
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
    let locus = match args.backend.as_str() {
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
    let hub = Arc::new(
        DynamicHub::new("locus")
            .register(locus.sessions)
            .register(locus.tabs)
            .register(locus.panes)
            .register(locus.info)
    );

    let rpc_converter = |arc| {
        DynamicHub::arc_into_rpc_module(arc)
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))
    };

    let mut builder = TransportServer::builder(hub, rpc_converter);

    if args.stdio {
        builder = builder.with_stdio();
        tracing::info!("Starting Locus in stdio mode");
    } else {
        builder = builder.with_websocket(args.port);
        tracing::info!("Locus listening on ws://127.0.0.1:{}", args.port);
    }

    builder.build().await?.serve().await
}
