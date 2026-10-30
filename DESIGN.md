# sitef-agent — design do agente TEF genérico (OSS)

Agente local que embrulha a biblioteca **CliSiTef** (Software Express) e a expõe por um
protocolo limpo (WebSocket), para que qualquer aplicação — web, desktop, mobile — faça
pagamentos TEF sem lidar com os detalhes da biblioteca nativa.

## Fronteira: o que é OSS e o que é privado
Dois repositórios, fronteira rígida:

| | `sitef-agent` (OSS, genérico) | `erp-adapter` (privado, seu) |
|---|---|---|
| Sabe de | Só a API da Software Express (CliSiTef) | As tabelas/API do seu ERP |
| Expõe | Protocolo WebSocket semântico | — (consome o agente) |
| Contém IP de terceiro? | Não — nem DLL, nem spec, nem tabelas verbatim | A lógica do seu ERP, privada |

> **Legal (modelo ACBr):** o OSS **chama** a CliSiTef que o usuário obtém da Software Express;
> **não** redistribui a DLL, os headers nem a spec (VRS-248 é confidencial), e **não** cola as
> tabelas de Comando/TipoCampo verbatim. O código implementa o comportamento com nomes próprios.
> Licença MIT/Apache-2.0 + disclaimer: "wrapper; homologação é entre o usuário e a Software
> Express/adquirente; sem garantia". Repo na conta pessoal/org neutra, identidade git por-repo.

## Arquitetura em camadas
```
 front (web/desktop)  ──WebSocket(JSON)──►  sitef-agent
                                              ├── transport  (servidor WS + sessões)
                                              ├── core       (máquina de estados do laço)
                                              └── clisitef   (trait) ──► NativeCliSiTef (FFI → DLL/.so)
                                                                     └► SimulatorCliSiTef (dev/test, sem hardware)
```

O **core** nunca chama a DLL direto: fala com o **trait `CliSiTef`**. Isso dá dois ganhos:
- **Testável sem maquininha** (impl Simulator com respostas roteirizadas) — essencial pra OSS,
  já que contribuidores não têm pinpad.
- **Portável** (a mesma lógica serve Windows `.dll` e Linux `.so`; só a impl Native muda).

## O trait CliSiTef (superfície mínima — as 6 funções)
```
trait CliSiTef {
    fn initialize(&self, cfg: Config) -> Result<()>;         // InicializaCliSiTef
    fn configure(&self, params: TxConfig) -> Result<()>;     // ConfiguraIntSiTefInterativo
    fn start(&self, fn_id: FuncId, value: Money, ...) -> i32;// IniciaFuncaoSiTefInterativo
    fn step(&self, answer: Option<Buf>, cont: Continue)      // ContinuaFuncaoSiTefInterativo
            -> Step;                                         //   → {ret, comando, tipo_campo, min, max, buffer}
    fn finish(&self, confirm: bool, data: ConfirmData);      // FinalizaTransacaoSiTefInterativo
    fn undo(&self, data: ConfirmData);                       // DesfazTransacaoSiTefInterativo
}
```

## Protocolo público (WebSocket, JSON) — onde a RE vira API limpa
O front abre uma sessão e troca mensagens semânticas. O agente **traduz** os `Comando`/`TipoCampo`
brutos para estes eventos — o front nunca vê inteiros da CliSiTef.

### Cliente → Agente (comandos)
| Tipo | Payload | Significado |
|---|---|---|
| `payment` | `{amount, kind?: credit\|debit\|pix\|voucher, installments?}` | inicia uma venda |
| `cancel` | `{amount, originalDate, originalNsu}` | cancelamento |
| `admin` | `{function}` | função gerencial |
| `answer` | `{value}` | responde a um `collect` |
| `choose` | `{index}` | responde a um `menu` |
| `yesno` | `{yes: bool}` | responde a um `confirm` |
| `abort` | — | interrompe o acesso ao pinpad (Continua = -1) |
| `settle` | `{confirm: bool}` | 2ª fase: confirma ou desfaz a transação autorizada |
| `resolvePending` | `{confirm: bool}` | resolve transação pendente detectada no boot |

### Agente → Cliente (eventos)
| Tipo | Payload | Origem (Comando) |
|---|---|---|
| `display` / `clearDisplay` | `{target: operator\|customer\|both, text}` | 1/2/3 · 11/12/13 |
| `menu` | `{title?, options:[{index,label,class?,type?}]}` | 21 / 42 |
| `collect` | `{field, kind: text\|money\|masked\|card\|date, min, max}` | 29/30/34/41 |
| `confirm` (sim/não) | `{text}` | 20 |
| `message` | `{text, waitKey: bool}` | 22 |
| `pinpad` | `{text}` | mensagem permanente do pinpad |
| `processing` | `{canAbort: true}` | 23 (pinpad lendo cartão/senha) |
| `datum` | `{field, value}` | 0 + TipoCampo (nsu, authorization, receipt…) |
| `result` | `{status: approved\|declined\|error, code, message, confirmation?}` | retorno 0 / <0 |
| `pending` | `{count}` | detectado no boot |

`field` em `datum`/`collect` é um **nome semântico** (ex.: `nsu`, `authorization`,
`receiptCustomer`, `receiptMerchant`, `cardBrand`, `installments`, `cardNumber`), mapeado
internamente do TipoCampo. O `confirmation` em `result` é guardado pelo agente para o `settle`.

## Máquina de estados (resumo)
`payment` → `start()` → laço `step()` enquanto retorno = 10000, emitindo os eventos acima e
consumindo `answer`/`choose`/`yesno`/`abort` → retorno 0 (aprovado) emite `result{approved}` →
front grava a venda e manda `settle{confirm:true}` (ou `{false}` em falha) → `finish()`/`undo()`.
No boot: consulta pendências → se houver, emite `pending{count}` e aguarda `resolvePending`.

## Decisões em aberto (do usuário)
- **Linguagem da impl:** recomendação **Rust** (memory-safe, binário único, `libloading` p/ a DLL,
  `axum`/`tokio-tungstenite` p/ WS). Alternativa: C# (se o alvo ficar Windows). Toolchain não está
  nesta máquina — instalar é decisão sua.
- **Criar o repo OSS** (conta `lucashs94`/org neutra, identidade git por-repo): só com OK explícito.

## Próximos passos (quando aprovado)
1. Scaffold do projeto (core + trait + Simulator) — buildável e testável sem hardware.
2. Impl `NativeCliSiTef` (FFI) atrás de feature flag, carregando a DLL/.so que o usuário fornece.
3. Impl do servidor WS + o protocolo acima.
4. Um `erp-adapter` privado consome o agente e integra com o seu ERP (lê o "a pagar", grava o resultado da transação).
