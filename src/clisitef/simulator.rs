//! Implementação roteirizada da CliSiTef, sem hardware. Reproduz uma venda de crédito
//! aprovada de ponta a ponta, para desenvolver e testar o agente e o front sem maquininha.

use super::{CliSiTef, Config, Continue, FuncId, Step};

#[derive(Default)]
pub struct Simulator {
    script: Vec<Step>,
    idx: usize,
}

impl Simulator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CliSiTef for Simulator {
    fn initialize(&mut self, _cfg: &Config) -> anyhow::Result<()> {
        Ok(())
    }

    fn configure(&mut self, _amount: &str, _func: FuncId) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self, _func: FuncId, amount: &str) -> i32 {
        self.idx = 0;
        // Sequência de uma venda de crédito aprovada: mensagem → acesso ao pinpad →
        // dados devolvidos (NSU, autorização, comprovante) → aprovado.
        self.script = vec![
            Step { ret: 10000, command: 3, buffer: format!("Processando R$ {amount}"), ..Default::default() },
            Step { ret: 10000, command: 23, ..Default::default() },
            Step { ret: 10000, command: 0, field_type: 133, buffer: "123456".into(), ..Default::default() },
            Step { ret: 10000, command: 0, field_type: 135, buffer: "789012".into(), ..Default::default() },
            Step { ret: 10000, command: 0, field_type: 121, buffer: "=== VIA CLIENTE ===".into(), ..Default::default() },
            Step { ret: 0, ..Default::default() },
        ];
        10000
    }

    fn step(&mut self, _answer: Option<&str>, _cont: Continue) -> Step {
        let s = self
            .script
            .get(self.idx)
            .cloned()
            .unwrap_or(Step { ret: 0, ..Default::default() });
        self.idx += 1;
        s
    }

    fn finish(&mut self, _confirm: bool) {}

    fn pending_count(&mut self) -> u32 {
        0
    }
}
