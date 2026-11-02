//! Model Context Protocol server for Shale.
//!
//! Tools call [`shale_core::Session`], the same job API the desktop window
//! uses for slicing and custom instructions. Stdio is the default transport.
//! `--http` serves streamable HTTP and refuses any address that is not a
//! loopback interface.

mod server;

use std::net::SocketAddr;

use anyhow::Context;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::ServiceExt;
use shale_core::{Engine, ResourcePaths, Session};
use tokio_util::sync::CancellationToken;

pub use server::ShaleMcp;

pub fn session_from_env() -> anyhow::Result<Session> {
    let paths = ResourcePaths::discover().context(
        "Cura resources were not found. Run scripts/fetch-resources.sh or set SHALE_CURA_DIR and SHALE_MATERIALS_DIR.",
    )?;
    let catalog = shale_core::Catalog::open(paths)?;
    let engine = Engine::from_env();
    Ok(Session::new(catalog, Box::new(engine)))
}

pub async fn serve_stdio(server: ShaleMcp) -> anyhow::Result<()> {
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .context("stdio transport failed")?;
    running.waiting().await?;
    Ok(())
}

pub async fn serve_http(server: ShaleMcp, address: SocketAddr) -> anyhow::Result<()> {
    if !address.ip().is_loopback() {
        anyhow::bail!("refusing to listen on {address}: the HTTP transport is localhost-only");
    }
    let token = CancellationToken::new();
    let child = token.child_token();
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(child),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("could not bind {address}"))?;
    tracing::info!("Shale MCP listening on http://{address}/mcp");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            token.cancel();
        })
        .await?;
    Ok(())
}

pub fn http_address(args: &[String]) -> anyhow::Result<Option<SocketAddr>> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--http" {
            let value = args
                .next()
                .context("--http needs an address, for example 127.0.0.1:7337")?;
            let address: SocketAddr = value
                .parse()
                .with_context(|| format!("could not parse HTTP address {value}"))?;
            if !address.ip().is_loopback() {
                anyhow::bail!("refusing non-loopback HTTP address {address}");
            }
            return Ok(Some(address));
        }
        anyhow::bail!("unknown argument {arg}. Usage: shale-mcp [--http 127.0.0.1:7337]");
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_address_is_loopback_only() {
        let address = http_address(&["--http".into(), "127.0.0.1:7337".into()])
            .unwrap()
            .unwrap();
        assert!(address.ip().is_loopback());
        assert_eq!(address.port(), 7337);
        let err = http_address(&["--http".into(), "0.0.0.0:7337".into()]).unwrap_err();
        assert!(err.to_string().contains("loopback"), "{err}");
    }
}
