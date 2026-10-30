//! Protocolo público agente↔cliente (JSON sobre WebSocket).
//!
//! Só mensagens **semânticas**: o cliente nunca vê os inteiros de `Comando`/`TipoCampo` da
//! CliSiTef. O agente traduz os códigos brutos nos eventos abaixo.

use serde::{Deserialize, Serialize};

/// Mensagens que o **cliente envia** ao agente.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMsg {
    /// Inicia uma venda.
    Payment {
        /// Valor como string decimal, ex.: "150.00".
        amount: String,
        #[serde(default)]
        kind: Option<PaymentKind>,
        #[serde(default)]
        installments: Option<u32>,
    },
    /// Cancelamento de uma transação anterior.
    Cancel {
        amount: String,
        /// Data original DDMMAAAA.
        original_date: String,
        original_nsu: String,
    },
    /// Função gerencial/administrativa.
    Admin { function: String },
    /// Resposta a um evento `collect`.
    Answer { value: String },
    /// Resposta a um evento `menu` (índice escolhido).
    Choose { index: u32 },
    /// Resposta a um evento `confirm` (sim/não).
    YesNo { yes: bool },
    /// Interrompe o acesso ao pinpad (Continua = -1).
    Abort,
    /// Segunda fase: confirma ou desfaz a transação autorizada.
    Settle { confirm: bool },
    /// Resolve uma transação pendente detectada na abertura.
    ResolvePending { confirm: bool },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentKind {
    Credit,
    Debit,
    Pix,
    Voucher,
}

/// Eventos que o **agente envia** ao cliente.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentEvent {
    /// Mensagem para um visor.
    Display { target: DisplayTarget, text: String },
    /// Remove a mensagem de um visor.
    ClearDisplay { target: DisplayTarget },
    /// Menu de opções; responder com `choose`.
    Menu {
        title: Option<String>,
        options: Vec<MenuOption>,
    },
    /// Coleta de um campo; responder com `answer`.
    Collect {
        field: String,
        kind: FieldKind,
        min: u16,
        max: u16,
    },
    /// Pergunta sim/não; responder com `yesNo`.
    Confirm { text: String },
    /// Mensagem que aguarda uma tecla do operador.
    Message { text: String, wait_key: bool },
    /// Mensagem permanente do pinpad.
    Pinpad { text: String },
    /// A biblioteca está acessando o pinpad; pode-se abortar (`abort`), sem delay.
    Processing { can_abort: bool },
    /// Um dado devolvido pela transação (nsu, autorização, comprovante, …).
    Datum { field: String, value: String },
    /// Resultado final da transação.
    Result {
        status: ResultStatus,
        code: i32,
        message: String,
    },
    /// Transações pendentes detectadas na abertura; responder com `resolvePending`.
    Pending { count: u32 },
    /// Erro de protocolo/sessão.
    Error { message: String },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayTarget {
    Operator,
    Customer,
    Both,
}

#[derive(Debug, Clone, Serialize)]
pub struct MenuOption {
    pub index: u32,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Money,
    Masked,
    Card,
    Date,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultStatus {
    Approved,
    Declined,
    Error,
}
