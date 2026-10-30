//! Máquina de estados do laço interativo: dirige a CliSiTef e **traduz** cada passo bruto
//! em um evento semântico do protocolo. Os inteiros de `Comando`/`TipoCampo` são o contrato
//! da API da biblioteca; os nomes e o tratamento aqui são interpretação própria deste projeto.

use anyhow::Result;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tracing::info;

use crate::clisitef::{CliSiTef, Config, Continue, FuncId};
use crate::protocol::*;

type Sink = SplitSink<WebSocketStream<TcpStream>, Message>;
type Source = SplitStream<WebSocketStream<TcpStream>>;

/// Códigos de `Comando` tratados (contrato da API da biblioteca).
mod cmd {
    pub const RETURN_DATUM: i32 = 0;
    pub const MSG_OPERATOR: i32 = 1;
    pub const MSG_CUSTOMER: i32 = 2;
    pub const MSG_BOTH: i32 = 3;
    pub const CLEAR_OPERATOR: i32 = 11;
    pub const CLEAR_CUSTOMER: i32 = 12;
    pub const CLEAR_BOTH: i32 = 13;
    pub const YESNO: i32 = 20;
    pub const MENU: i32 = 21;
    pub const MESSAGE_WAIT: i32 = 22;
    pub const CAN_INTERRUPT: i32 = 23;
    pub const COLLECT_SILENT: i32 = 29;
    pub const COLLECT: i32 = 30;
    pub const COLLECT_MONEY: i32 = 34;
    pub const COLLECT_MASKED: i32 = 41;
}

pub struct Session {
    tef: Box<dyn CliSiTef>,
    cfg: Config,
    out: Sink,
    inp: Source,
}

impl Session {
    pub fn new(tef: Box<dyn CliSiTef>, cfg: Config, out: Sink, inp: Source) -> Self {
        Self { tef, cfg, out, inp }
    }

    pub async fn run(&mut self) -> Result<()> {
        self.tef.initialize(&self.cfg)?;

        // Coleta de pendências na abertura.
        let pend = self.tef.pending_count();
        if pend > 0 {
            self.send(AgentEvent::Pending { count: pend }).await?;
            if let Some(ClientMsg::ResolvePending { confirm }) = self.next_client().await? {
                self.tef.finish(confirm);
            }
        }

        while let Some(msg) = self.next_client().await? {
            match msg {
                ClientMsg::Payment { amount, kind, .. } => {
                    info!(?kind, %amount, "iniciando pagamento");
                    self.run_payment(&amount).await?;
                }
                ClientMsg::Settle { confirm } => self.tef.finish(confirm),
                ClientMsg::Cancel { .. } | ClientMsg::Admin { .. } => {
                    self.send(AgentEvent::Error {
                        message: "operação ainda não implementada no scaffold".into(),
                    })
                    .await?;
                }
                _ => {
                    self.send(AgentEvent::Error {
                        message: "comando fora de contexto".into(),
                    })
                    .await?;
                }
            }
        }
        Ok(())
    }

    async fn run_payment(&mut self, amount: &str) -> Result<()> {
        self.tef.configure(amount, FuncId::Payment)?;
        let _ = self.tef.start(FuncId::Payment, amount);

        let mut answer: Option<String> = None;
        let mut cont = Continue::Go;

        loop {
            let step = self.tef.step(answer.take().as_deref(), cont);
            cont = Continue::Go;

            if step.ret == 0 {
                self.send(AgentEvent::Result {
                    status: ResultStatus::Approved,
                    code: 0,
                    message: "Aprovado".into(),
                })
                .await?;
                return Ok(());
            }
            if step.ret < 0 {
                self.send(AgentEvent::Result {
                    status: ResultStatus::Error,
                    code: step.ret,
                    message: "Transação não concluída".into(),
                })
                .await?;
                return Ok(());
            }

            // step.ret == 10000 → tratar o comando.
            match step.command {
                cmd::MSG_OPERATOR => {
                    self.send(AgentEvent::Display { target: DisplayTarget::Operator, text: step.buffer }).await?
                }
                cmd::MSG_CUSTOMER => {
                    self.send(AgentEvent::Display { target: DisplayTarget::Customer, text: step.buffer }).await?
                }
                cmd::MSG_BOTH => {
                    self.send(AgentEvent::Display { target: DisplayTarget::Both, text: step.buffer }).await?
                }
                cmd::CLEAR_OPERATOR => {
                    self.send(AgentEvent::ClearDisplay { target: DisplayTarget::Operator }).await?
                }
                cmd::CLEAR_CUSTOMER => {
                    self.send(AgentEvent::ClearDisplay { target: DisplayTarget::Customer }).await?
                }
                cmd::CLEAR_BOTH => {
                    self.send(AgentEvent::ClearDisplay { target: DisplayTarget::Both }).await?
                }
                cmd::RETURN_DATUM => {
                    self.send(AgentEvent::Datum { field: field_name(step.field_type), value: step.buffer }).await?
                }
                cmd::MESSAGE_WAIT => {
                    self.send(AgentEvent::Message { text: step.buffer, wait_key: true }).await?
                }
                cmd::CAN_INTERRUPT => {
                    // Não bloquear nem inserir delay aqui (o pinpad está acessando o periférico).
                    self.send(AgentEvent::Processing { can_abort: true }).await?
                }
                cmd::YESNO => {
                    self.send(AgentEvent::Confirm { text: step.buffer }).await?;
                    match self.next_client().await? {
                        Some(ClientMsg::YesNo { yes }) => {
                            answer = Some(if yes { "0" } else { "1" }.into())
                        }
                        _ => cont = Continue::Abort,
                    }
                }
                cmd::MENU => {
                    let options = parse_menu(&step.buffer);
                    self.send(AgentEvent::Menu { title: None, options }).await?;
                    match self.next_client().await? {
                        Some(ClientMsg::Choose { index }) => answer = Some(index.to_string()),
                        _ => cont = Continue::Abort,
                    }
                }
                cmd::COLLECT | cmd::COLLECT_MONEY | cmd::COLLECT_MASKED | cmd::COLLECT_SILENT => {
                    let kind = match step.command {
                        cmd::COLLECT_MONEY => FieldKind::Money,
                        cmd::COLLECT_MASKED => FieldKind::Masked,
                        _ => FieldKind::Text,
                    };
                    self.send(AgentEvent::Collect {
                        field: field_name(step.field_type),
                        kind,
                        min: step.min,
                        max: step.max,
                    })
                    .await?;
                    match self.next_client().await? {
                        Some(ClientMsg::Answer { value }) => answer = Some(value),
                        _ => cont = Continue::Abort,
                    }
                }
                _ => { /* comando não tratado: ignora e segue o laço */ }
            }
        }
    }

    async fn send(&mut self, ev: AgentEvent) -> Result<()> {
        let txt = serde_json::to_string(&ev)?;
        self.out.send(Message::Text(txt)).await?;
        Ok(())
    }

    /// Lê a próxima mensagem textual válida do cliente; `None` encerra a conexão.
    async fn next_client(&mut self) -> Result<Option<ClientMsg>> {
        while let Some(frame) = self.inp.next().await {
            match frame? {
                Message::Text(t) => match serde_json::from_str::<ClientMsg>(&t) {
                    Ok(m) => return Ok(Some(m)),
                    Err(e) => {
                        self.send(AgentEvent::Error {
                            message: format!("mensagem inválida: {e}"),
                        })
                        .await?;
                    }
                },
                Message::Close(_) => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }
}

/// Nome semântico de um campo devolvido/coletado. Subconjunto; estender conforme necessário.
fn field_name(t: i32) -> String {
    let name = match t {
        100 => "paymentModality",
        105 => "dateTime",
        121 => "receiptCustomer",
        122 => "receiptMerchant",
        133 => "nsuSitef",
        134 => "nsuHost",
        135 => "authorization",
        156 => "institutionName",
        158 => "network",
        160 => "couponNumber",
        505 => "installments",
        _ => return format!("field{t}"),
    };
    name.to_string()
}

/// Converte o buffer de menu "1:texto;2:texto;…" em opções.
fn parse_menu(buf: &str) -> Vec<MenuOption> {
    buf.split(';')
        .filter(|s| !s.is_empty())
        .filter_map(|opt| {
            let (idx, label) = opt.split_once(':')?;
            Some(MenuOption {
                index: idx.trim().parse().ok()?,
                label: label.to_string(),
            })
        })
        .collect()
}
