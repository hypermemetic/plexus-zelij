use clap::Parser;
use plexus_locus::{Locus, Zellij};
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

    // Build the Locus activation with Zellij backend
    let locus = Locus::new(Zellij::new());

    // Wrap in a DynamicHub so it's a standalone Plexus RPC server
    let hub = Arc::new(
        DynamicHub::new("locus")
            .register(locus)
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
