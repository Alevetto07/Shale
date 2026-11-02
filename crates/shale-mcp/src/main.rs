use anyhow::Context;
use shale_mcp::{http_address, serve_http, serve_stdio, session_from_env, ShaleMcp};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let address = http_address(&args)?;
    let session = session_from_env()?;
    let server = ShaleMcp::new(session);
    if let Some(address) = address {
        serve_http(server, address).await
    } else {
        serve_stdio(server)
            .await
            .context("MCP stdio server stopped")
    }
}
