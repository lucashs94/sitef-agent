//! Abstração sobre a biblioteca CliSiTef interativa.
//!
//! O `core`/`session` nunca chama a biblioteca direto: fala com o trait [`CliSiTef`].
//! Isso permite testar sem hardware (impl [`simulator::Simulator`]) e portar entre
//! Windows (`.dll`) e Linux (`.so`) trocando só a impl nativa.

pub mod simulator;

#[cfg(feature = "native")]
pub mod native;

/// Configuração para abrir uma sessão CliSiTef.
#[allow(dead_code)] // campos lidos pela impl nativa; o simulador os ignora.
#[derive(Debug, Clone)]
pub struct Config {
    /// CodigoLoja.
    pub store_id: String,
    /// NumeroTerminal.
    pub terminal: String,
    /// EnderecoIP do servidor SiTef.
    pub server_ip: String,
}

/// Qual função interativa iniciar.
#[derive(Debug, Clone, Copy)]
pub enum FuncId {
    Payment,
    Cancellation,
    Admin,
}

/// Instrução "Continua" realimentada a cada passo do laço.
#[derive(Debug, Clone, Copy)]
pub enum Continue {
    /// Segue a transação.
    Go,
    /// Encerra a transação (-1).
    Abort,
    /// Volta ao campo anterior (1).
    Back,
}

impl Continue {
    pub fn code(self) -> i32 {
        match self {
            Continue::Go => 0,
            Continue::Abort => -1,
            Continue::Back => 1,
        }
    }
}

/// Resultado de uma iteração do laço interativo (espelha um retorno da rotina de continuação).
#[derive(Debug, Clone, Default)]
pub struct Step {
    /// 0 = concluído/aprovado, 10000 = continuar o laço, < 0 = erro.
    pub ret: i32,
    /// Próxima ação para a aplicação.
    pub command: i32,
    /// Qual campo (quando `command` = 0 ou é uma coleta).
    pub field_type: i32,
    pub min: u16,
    pub max: u16,
    /// Área de transferência de dados com a biblioteca.
    pub buffer: String,
}

/// Contrato mínimo da CliSiTef interativa (as seis funções essenciais).
pub trait CliSiTef: Send {
    fn initialize(&mut self, cfg: &Config) -> anyhow::Result<()>;
    fn configure(&mut self, amount: &str, func: FuncId) -> anyhow::Result<()>;
    /// Dispara a função; retorna 10000 para iniciar o laço, 0 se já concluiu.
    fn start(&mut self, func: FuncId, amount: &str) -> i32;
    /// Uma iteração do laço: consome a resposta anterior e a instrução `Continua`.
    fn step(&mut self, answer: Option<&str>, cont: Continue) -> Step;
    /// Segunda fase: confirma (true) ou desfaz (false) a transação autorizada.
    fn finish(&mut self, confirm: bool);
    /// Quantidade de transações pendentes (consultada na abertura).
    fn pending_count(&mut self) -> u32;
}
