//! Servidor WebSocket: aceita conexões e cria uma sessão por cliente.

use anyhow::Result;
use futures_util::StreamExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

use crate::clisitef::{CliSiTef, Config};
use crate::session::Session;

pub async fn serve(addr: &str) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("sitef-agent ouvindo em ws://{addr}");
    loop {
        let (stream, peer) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle(stream).await {
                warn!("sessão {peer} encerrada: {e}");
            }
        });
    }
}

async fn handle(stream: TcpStream) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (write, read) = ws.split();

    // Configuração do terminal (viria de arquivo/variáveis de ambiente).
    let cfg = Config {
        store_id: std::env::var("SITEF_LOJA").unwrap_or_else(|_| "00000000".into()),
        terminal: std::env::var("SITEF_TERMINAL").unwrap_or_else(|_| "SITEF001".into()),
        server_ip: std::env::var("SITEF_SERVIDOR").unwrap_or_else(|_| "127.0.0.1".into()),
    };

    let tef = build_backend()?;

    let mut session = Session::new(tef, cfg, write, read);
    session.run().await
}

/// Seleciona a implementação da CliSiTef.
///
/// Sem a feature `native`: sempre o simulador (dev/test, sem hardware).
/// Com a feature `native`: carrega a DLL/.so real do caminho em `SITEF_LIB`; se a variável
/// não estiver definida, cai no simulador e avisa.
#[cfg(not(feature = "native"))]
fn build_backend() -> Result<Box<dyn CliSiTef>> {
    use crate::clisitef::simulator::Simulator;
    Ok(Box::new(Simulator::new()))
}

#[cfg(feature = "native")]
fn build_backend() -> Result<Box<dyn CliSiTef>> {
    use crate::clisitef::native::NativeCliSiTef;
    use crate::clisitef::simulator::Simulator;
    match std::env::var("SITEF_LIB") {
        Ok(path) => {
            info!("carregando biblioteca CliSiTef nativa de '{path}'");
            Ok(Box::new(NativeCliSiTef::load(&path)?))
        }
        Err(_) => {
            warn!("SITEF_LIB não definida: usando o simulador mesmo com a feature `native`");
            Ok(Box::new(Simulator::new()))
        }
    }
}
