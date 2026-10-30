# sitef-agent

Agente local que embrulha a biblioteca **CliSiTef** (Software Express) e a expõe por um
protocolo **WebSocket** limpo e genérico. Qualquer aplicação — web, desktop ou mobile — faz
pagamentos TEF conversando com o agente, sem lidar com a biblioteca nativa nem com o laço
interativo da CliSiTef.

> **Por que existe:** o pinpad fica numa porta serial/USB da máquina do caixa; nenhum navegador
> fala com ele direto. O agente roda nessa máquina, dona do pinpad, e entrega uma API moderna
> para o resto do sistema.

## Estado
O caminho `simulator` (padrão) roda uma venda de crédito aprovada de ponta a ponta, sem hardware
— compila e passa o teste end-to-end. O binding `native` (feature `native`) está **implementado**:
carrega a DLL/.so real e chama as funções interativas da CliSiTef via FFI. Falta apenas a
**validação em máquina com pinpad + biblioteca + ambiente SiTef** — TEF só se valida com hardware.

## Arquitetura
```
 front (web/desktop)  ──WebSocket(JSON)──►  sitef-agent
                                              ├── transport  (servidor WS + sessões)
                                              ├── session    (máquina de estados do laço)
                                              └── clisitef   (trait)
                                                    ├── simulator  (dev/test, sem hardware)
                                                    └── native     (FFI → DLL/.so)  [feature "native"]
```
O `session` nunca chama a biblioteca direto: fala com o trait `CliSiTef`. Isso torna o agente
testável sem maquininha e portável entre Windows (`.dll`) e Linux (`.so`).

## Build e execução
Requer o toolchain Rust (`rustup`, stable).
```sh
cargo run                 # simulador (padrão), escuta em ws://127.0.0.1:9099
cargo build --release     # binário
cargo build --features native --no-default-features   # com o binding nativo
```
Variáveis: `SITEF_AGENT_ADDR` (endereço), `SITEF_LOJA`, `SITEF_TERMINAL`, `SITEF_SERVIDOR`.
Com a feature `native`: `SITEF_LIB` (caminho da DLL/.so; se ausente, cai no simulador),
`SITEF_MODALITY_CANCEL` / `SITEF_MODALITY_ADMIN` (códigos de modalidade da sua integração),
`SITEF_SYMBOL_SUFFIX` (override dos nomes de símbolo, normalmente desnecessário).

### 32 vs 64 bit
Um processo só carrega uma biblioteca da **mesma arquitetura**: um binário 64-bit não abre a
`clisitef32I.dll`. Portanto a escolha é no **alvo de compilação**, não em runtime — o mesmo
código-fonte serve para os dois; os nomes/convenção dos símbolos são resolvidos automaticamente.

| Biblioteca da sua máquina | Alvo de build |
|---|---|
| `CliSiTef64I.dll` (64-bit) | `x86_64-pc-windows-msvc` (ou `-gnu`) |
| `clisitef32I.dll` (32-bit) | `i686-pc-windows-msvc` (ou `-gnu`) |
| `.so` no Linux | alvo nativo da máquina |

```sh
cargo build --release --features native --no-default-features --target i686-pc-windows-gnu
```

### Teste rápido (com o simulador)
Conecte um cliente WebSocket a `ws://127.0.0.1:9099` e envie:
```json
{"type":"payment","amount":"150.00","kind":"credit"}
```
O agente responde com a sequência de eventos (`display`, `processing`, `datum` de NSU/autorização/
comprovante) e finaliza com `result` aprovado. Confirme com:
```json
{"type":"settle","confirm":true}
```

## Protocolo (resumo)
**Cliente → Agente:** `payment`, `cancel`, `admin`, `answer`, `choose`, `yesNo`, `abort`,
`settle`, `resolvePending`.
**Agente → Cliente:** `display`, `clearDisplay`, `menu`, `collect`, `confirm`, `message`,
`pinpad`, `processing`, `datum`, `result`, `pending`, `error`.

Os eventos são **semânticos**: o cliente nunca vê os inteiros de `Comando`/`TipoCampo` da
CliSiTef — o agente os traduz.

## Aviso legal
Este projeto **não** inclui nem redistribui a biblioteca CliSiTef, seus cabeçalhos ou a
especificação do fabricante — ela deve ser obtida junto à **Software Express**. O agente apenas a
**chama**. A homologação com as adquirentes é responsabilidade do usuário, entre ele e a Software
Express/adquirente. Software distribuído **sem garantias** (ver `LICENSE`).

## Roadmap
- Validar o binding `native` em máquina com pinpad + biblioteca + ambiente SiTef.
- Cancelamento e funções gerenciais (fluxo no `session`, códigos de modalidade por integração).
- Confirmação/desfazimento e persistência local de pendências.
- Protocolo PIX/carteiras digitais (campos específicos).
