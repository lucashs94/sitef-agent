//! sitef-agent — agente TEF local sobre a CliSiTef, exposto por WebSocket.
//!
//! Arquitetura em camadas: `transport` (servidor WS) → `session` (máquina de estados do
//! laço interativo) → `clisitef` (trait, com impl `simulator` e, opcional, `native`).

mod clisitef;
mod protocol;
mod session;
mod transport;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sitef_agent=info")),
        )
        .init();

    let addr = std::env::var("SITEF_AGENT_ADDR").unwrap_or_else(|_| "127.0.0.1:9099".into());
    transport::serve(&addr).await
}
