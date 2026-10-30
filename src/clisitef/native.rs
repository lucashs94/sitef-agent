//! Binding nativo (feature `native`). Carrega a biblioteca CliSiTef que o **usuário fornece**
//! (`clisitef32I.dll` / `CliSiTef64I.dll` no Windows, `libclisitef.so` no Linux) — este projeto
//! não redistribui a biblioteca nem a especificação do fabricante, apenas a **chama**.
//!
//! As assinaturas abaixo são as de ABI pública da CliSiTef interativa (as mesmas declaradas por
//! projetos open-source de referência, como o ACBr).
//!
//! ## 32 vs 64 bit
//! Um processo só carrega uma biblioteca da **mesma arquitetura**: um `.exe` 64-bit NÃO abre a
//! `clisitef32I.dll` (32-bit) e vice-versa. Logo, 32 vs 64 é escolhido pelo **alvo de compilação**
//! do agente, não por um flag em runtime — compila-se um binário para cada caso, deste mesmo fonte.
//!
//! A convenção e os nomes são resolvidos automaticamente por [`sym_name`]:
//! - **64-bit** (`CliSiTef64I.dll`, `.so`): há uma única convenção; usamos os nomes puros com
//!   `extern "C"`.
//! - **32-bit** (`clisitef32I.dll`): usamos os pontos de entrada com sufixo **`Cdecl`**, que são
//!   `cdecl` (= `extern "C"` no i686) — evitando os nomes `stdcall` decorados (`_Nome@N`).
//!
//! Override: a env `SITEF_SYMBOL_SUFFIX` força o sufixo (ex.: vazio, ou `Cdecl`) se a sua versão
//! da biblioteca exportar nomes diferentes.
//!
//! A biblioteca troca texto em **Latin-1 (ISO-8859-1)**, não UTF-8: a conversão é feita aqui.

use super::{CliSiTef, Config, Continue, FuncId, Step};
use anyhow::{anyhow, bail, Context, Result};
use libloading::Library;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tamanho do buffer bidirecional trocado com a biblioteca a cada passo do laço.
const BUF_SIZE: usize = 20_000;

// Assinaturas das funções exportadas pela CliSiTef interativa.
type ConfiguraFn =
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, c_int) -> c_int;
type IniciaFn = unsafe extern "C" fn(
    c_int,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
) -> c_int;
type ContinuaFn = unsafe extern "C" fn(
    *mut c_int,
    *mut c_int,
    *mut c_int,
    *mut c_int,
    *mut c_char,
    c_int,
    c_int,
) -> c_int;
type FinalizaFn =
    unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_char) -> c_int;
type PendentesFn = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;

/// Metadados fiscais de uma transação (preenchidos ao iniciar, reusados ao finalizar).
#[derive(Default, Clone)]
struct TxContext {
    coupon: String,
    date: String,
    time: String,
}

/// Binding sobre a biblioteca nativa. Os ponteiros de função têm validade atrelada à `Library`,
/// por isso ela é mantida viva no mesmo struct (`_lib`) e nunca liberada antes deles.
pub struct NativeCliSiTef {
    _lib: Library,
    configura: ConfiguraFn,
    inicia: IniciaFn,
    continua: ContinuaFn,
    finaliza: FinalizaFn,
    pendentes: Option<PendentesFn>,
    cfg: Config,
    tx: TxContext,
}

impl NativeCliSiTef {
    /// Carrega a biblioteca no caminho dado (a DLL/.so que o usuário obteve da Software Express)
    /// e resolve os símbolos interativos.
    pub fn load(path: &str) -> Result<Self> {
        // SAFETY: carregar uma biblioteca dinâmica roda o inicializador dela; confiamos na DLL
        // oficial da CliSiTef fornecida pelo operador.
        let lib = unsafe { Library::new(path) }
            .with_context(|| format!("não foi possível carregar a biblioteca '{path}'"))?;

        // SAFETY: os tipos declarados acima correspondem à ABI pública da CliSiTef interativa.
        // Os ponteiros resolvidos permanecem válidos enquanto `lib` (movida para o struct) existir.
        unsafe {
            let configura: ConfiguraFn = *lib
                .get(&sym_name("ConfiguraIntSiTefInterativo"))
                .context("símbolo ConfiguraIntSiTefInterativo ausente")?;
            let inicia: IniciaFn = *lib
                .get(&sym_name("IniciaFuncaoSiTefInterativo"))
                .context("símbolo IniciaFuncaoSiTefInterativo ausente")?;
            let continua: ContinuaFn = *lib
                .get(&sym_name("ContinuaFuncaoSiTefInterativo"))
                .context("símbolo ContinuaFuncaoSiTefInterativo ausente")?;
            let finaliza: FinalizaFn = *lib
                .get(&sym_name("FinalizaFuncaoSiTefInterativo"))
                .context("símbolo FinalizaFuncaoSiTefInterativo ausente")?;
            // Opcional: nem toda versão exporta a consulta de pendências. Tenta o nome resolvido
            // (com sufixo de arquitetura) e, se faltar, o nome puro.
            let pendentes: Option<PendentesFn> = lib
                .get::<PendentesFn>(&sym_name("ObtemQuantidadeTransacoesPendentes"))
                .or_else(|_| lib.get::<PendentesFn>(b"ObtemQuantidadeTransacoesPendentes\0"))
                .ok()
                .map(|s| *s);

            Ok(Self {
                _lib: lib,
                configura,
                inicia,
                continua,
                finaliza,
                pendentes,
                cfg: Config {
                    store_id: String::new(),
                    terminal: String::new(),
                    server_ip: String::new(),
                },
                tx: TxContext::default(),
            })
        }
    }
}

impl CliSiTef for NativeCliSiTef {
    fn initialize(&mut self, cfg: &Config) -> Result<()> {
        self.cfg = cfg.clone();
        let ip = cstr(&cfg.server_ip)?;
        let loja = cstr(&cfg.store_id)?;
        let term = cstr(&cfg.terminal)?;
        // SAFETY: ponteiros válidos durante a chamada; a lib copia o conteúdo.
        let ret = unsafe { (self.configura)(ip.as_ptr(), loja.as_ptr(), term.as_ptr(), 0) };
        if ret != 0 {
            bail!("ConfiguraIntSiTefInterativo falhou (código {ret})");
        }
        Ok(())
    }

    fn configure(&mut self, _amount: &str, _func: FuncId) -> Result<()> {
        // Na CliSiTef interativa a configuração por-valor é feita direto em `start`
        // (IniciaFuncaoSiTefInterativo); nada a fazer aqui.
        Ok(())
    }

    fn start(&mut self, func: FuncId, amount: &str) -> i32 {
        let modality = match modality_code(func) {
            Some(m) => m,
            None => return -1, // modalidade não configurada: trata como erro no laço do session.
        };

        // Metadados fiscais: cupom + data/hora atuais, reusados na finalização.
        let (date, time) = now_fiscal();
        self.tx = TxContext {
            coupon: next_coupon(),
            date,
            time,
        };

        // Monta os argumentos C; se algum tiver NUL interno, aborta (erro).
        let args = (|| -> Result<_> {
            Ok((
                cstr(amount)?,
                cstr(&self.tx.coupon)?,
                cstr(&self.tx.date)?,
                cstr(&self.tx.time)?,
                // Operador de caixa (ex.: "TOTEM"); configurável, default genérico.
                cstr(&std::env::var("SITEF_OPERADOR").unwrap_or_else(|_| "1".into()))?,
                cstr("")?, // restrições
            ))
        })();
        let (valor, cupom, data, hora, oper, restr) = match args {
            Ok(a) => a,
            Err(_) => return -1,
        };

        // SAFETY: todos os ponteiros são válidos durante a chamada.
        unsafe {
            (self.inicia)(
                modality,
                valor.as_ptr(),
                cupom.as_ptr(),
                data.as_ptr(),
                hora.as_ptr(),
                oper.as_ptr(),
                restr.as_ptr(),
            )
        }
    }

    fn step(&mut self, answer: Option<&str>, cont: Continue) -> Step {
        let mut buf = vec![0u8; BUF_SIZE];
        // Buffer bidirecional: quando respondemos uma coleta, a resposta vai escrita no buffer.
        if let Some(a) = answer {
            let bytes = to_latin1(a);
            let n = bytes.len().min(BUF_SIZE - 1);
            buf[..n].copy_from_slice(&bytes[..n]);
            // buf[n] permanece 0 (terminador).
        }

        let mut comando: c_int = 0;
        let mut tipo: c_int = 0;
        let mut tmin: c_int = 0;
        let mut tmax: c_int = 0;

        // SAFETY: os quatro inteiros são out-params válidos; o buffer tem BUF_SIZE bytes.
        let ret = unsafe {
            (self.continua)(
                &mut comando,
                &mut tipo,
                &mut tmin,
                &mut tmax,
                buf.as_mut_ptr() as *mut c_char,
                BUF_SIZE as c_int,
                cont.code(),
            )
        };

        Step {
            ret,
            command: comando,
            field_type: tipo,
            min: tmin.max(0) as u16,
            max: tmax.max(0) as u16,
            buffer: from_latin1(&buf),
        }
    }

    fn finish(&mut self, confirm: bool) {
        let cupom = cstr(&self.tx.coupon).unwrap_or_default();
        let data = cstr(&self.tx.date).unwrap_or_default();
        let hora = cstr(&self.tx.time).unwrap_or_default();
        // 1 = confirma a transação; 0 = desfaz.
        // SAFETY: ponteiros válidos durante a chamada.
        unsafe {
            (self.finaliza)(
                confirm as c_int,
                cupom.as_ptr(),
                data.as_ptr(),
                hora.as_ptr(),
            );
        }
    }

    fn pending_count(&mut self) -> u32 {
        let Some(f) = self.pendentes else {
            return 0;
        };
        let (Ok(loja), Ok(term)) = (cstr(&self.cfg.store_id), cstr(&self.cfg.terminal)) else {
            return 0;
        };
        // SAFETY: ponteiros válidos durante a chamada.
        let n = unsafe { f(loja.as_ptr(), term.as_ptr()) };
        n.max(0) as u32
    }
}

/// Código de modalidade para `IniciaFuncaoSiTefInterativo`.
///
/// Venda genérica = `0` (a própria SiTef apresenta o menu de modalidade) — valor público.
/// Cancelamento/administrativo variam por integração: lidos de variável de ambiente para não
/// embutir no código valores da especificação proprietária.
fn modality_code(func: FuncId) -> Option<i32> {
    match func {
        FuncId::Payment => Some(0),
        FuncId::Cancellation => env_i32("SITEF_MODALITY_CANCEL"),
        FuncId::Admin => env_i32("SITEF_MODALITY_ADMIN"),
    }
}

fn env_i32(key: &str) -> Option<i32> {
    std::env::var(key).ok()?.trim().parse().ok()
}

/// Número de cupom sequencial (base local). Em produção, passe o cupom fiscal real da venda.
fn next_coupon() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{}", base.wrapping_add(n) % 1_000_000)
}

/// Data (DDMMAAAA) e hora (HHMMSS) atuais em horário UTC, dep-free.
fn now_fiscal() -> (String, String) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days as i64);
    (
        format!("{:02}{:02}{:04}", d, m, y),
        format!("{:02}{:02}{:02}", hh, mm, ss),
    )
}

/// Converte dias desde a época Unix em (ano, mês, dia). Algoritmo de Howard Hinnant (domínio público).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Nome do símbolo a resolver, terminado em NUL, com o sufixo de convenção adequado.
///
/// Ordem de decisão: env `SITEF_SYMBOL_SUFFIX` (override explícito) → sufixo por arquitetura
/// (`Cdecl` em 32-bit, vazio em 64-bit). Ambos os ramos são compilados em qualquer alvo
/// (via `cfg!`), então o fonte vale para os dois sem duplicação.
fn sym_name(base: &str) -> Vec<u8> {
    let mut v = base.as_bytes().to_vec();
    if let Ok(suffix) = std::env::var("SITEF_SYMBOL_SUFFIX") {
        v.extend_from_slice(suffix.as_bytes());
    } else if cfg!(target_pointer_width = "32") {
        v.extend_from_slice(b"Cdecl");
    }
    v.push(0);
    v
}

/// String Rust → `CString` em Latin-1 (como a CliSiTef espera). Falha se houver NUL interno.
fn cstr(s: &str) -> Result<CString> {
    CString::new(to_latin1(s)).map_err(|_| anyhow!("texto com NUL interno não pode ir para a C API"))
}

/// Codifica para Latin-1 (ISO-8859-1); caracteres fora do intervalo viram '?'.
fn to_latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| if (c as u32) <= 0xFF { c as u8 } else { b'?' })
        .collect()
}

/// Decodifica Latin-1 até o primeiro NUL. Cada byte 0x00–0xFF mapeia direto ao code point Unicode.
fn from_latin1(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf[..end].iter().map(|&b| b as char).collect()
}
