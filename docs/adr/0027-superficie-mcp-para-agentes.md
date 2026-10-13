# ADR-0027 — A superfície MCP é do próprio servidor, e uma tool é um pedido do BFF com a mesma autorização

**Estado:** Proposto · **Data:** 2026-10-13 ·
**Contexto:** expor o Meet a agentes (Claude Desktop, Claude Code, outro cliente
MCP) com tools especializadas — acta, legendas traduzidas, tradução de áudio ao
vivo, ajuda na edição, recortes e *shorts* — sem que um agente alcance reuniões
que não são da pessoa autenticada nem da organização dela ·
**Assenta em:** [ADR-0004](0004-organizacao-alvo-do-backend.md) §4 (uma
superfície por público), [ADR-0008](0008-papeis-capacidades-e-ambito.md)
(capacidades e âmbito) e [ADR-0011](0011-chaves-de-acesso-e-sessoes.md)
(sessões revogáveis) · **Credencial:** [ADR-0028](0028-oauth-2-1-para-clientes-mcp.md).

## Contexto

Medido na `origin/develop` (`dec0c207`) a 2026-10-13, antes de desenhar:

1. **Não há nada de MCP.** `git grep -i mcp` na `develop` devolve três `.wasm`
   do MediaPipe e quatro PNG de template. Zero linhas de código.

2. **Metade do que o pedido quer já existe, e vive no BFF.** Das 302 rotas de
   `server/src/lib.rs`:
   - **acta** — `ai::summarize_minutes` (`ai.rs`, 1128 linhas) com fila, prazo
     de reserva e tentativas (`mom_summary_due`);
     `PUT /api/meetings/{id}/minutes`, `GET/PUT /api/rooms/{code}/minutes`,
     `POST …/minutes/summary`, e na v1 `GET /api/v1/meetings/{id}/minutes` —
     esta última com o `SELECT` dentro do handler (`apikeys.rs:779`), que é a
     primeira coisa que o §3 obriga a arrumar;
   - **legendas traduzidas** — `recording_captions.rs` (775 linhas) com três
     origens na mesma tabela (`upload`, `transcript`, `translation`), WebVTT
     servido ao leitor, progresso da tradução em segundo plano, e
     `POST /api/ai/translations` para a legenda ao vivo;
   - **ajuda na edição** — `ai_studio.rs` (910 linhas): resumo com capítulos,
     texto de publicação e bordões a cortar, com tecto de concorrência por
     organização (`AI_STUDIO_CONCURRENCY_PER_ORG`); e
     `recording_chapters.rs` (787 linhas) com geração automática, estado da
     última geração e a regra de que uma máquina nunca apaga um capítulo
     escrito à mão.

3. **A outra metade não existe.** Não há tabela, fila nem render de **recortes**
   (`git grep -il clip server/migrations` = vazio); `stream_destinations.rs` é
   RTMP **ao vivo** para plataformas, não publicação de vídeo curto; e não há
   **TTS** em lado nenhum (`piper`, `coqui`, `text-to-speech` não aparecem em
   `server`, `web/src`, `ai-worker`, `whisper-server` nem `deploy`). A tradução
   ao vivo que existe é de **texto**: o browser transcreve, o servidor limpa com
   DLP (`dlp::clean_caption`) e difunde `ServerMsg::Transcript`.

4. **A chave `dlx_` não serve para isto.** O catálogo de escopos tem **sete**
   entradas (`Scope::ALL` em `delonix-meet-domain/src/identity/api_key.rs`) e
   nenhuma abre legendas, capítulos ou Estúdio; e a gestão de chaves é «admin da
   org, autenticada por sessão» (`apikeys.rs`) — um membro normal não consegue
   emitir a sua. Uma chave `dlx_` é credencial de **integração**, age como quem
   a criou, e é por isso o oposto do que o pedido pede.

5. **A autorização que o pedido quer já está escrita, e não é só pertença.** O
   motor de capacidades do ADR-0008 (`org::decide`) decide por capacidade e por
   `ResourceScope`; `recordings` tem visibilidade própria (`readable`, `library`,
   partilhas, link público); e a catraca da arquitectura recusa comparar o TEXTO
   do papel (`verificacoes_papel_por_string_fora_de_org_rs`). Quem reescrever
   estas regras num processo separado reescreve-as mal.

6. **Dois portões guardam exactamente esta fronteira.**
   `scripts/check-route-auth.sh` exige extractor de autenticação em toda a rota
   (existe porque `/api/mls/*` esteve aberto a responder «delivered» a qualquer
   pessoa — R41); `scripts/check-isolamento-cobertura.sh` exige que toda a rota
   de organização seja exercitada por `web/e2e/isolamento.mjs` (existe porque 6
   de 19 não o eram — R95). Os dois correm no CI.

## Decisão

### 1. A superfície MCP é do próprio binário, em `/api/mcp`

Não há serviço novo. É a **quinta superfície** do ADR-0004 §4 (BFF, v1 do
inquilino, operador, integrações, interna) e segue a regra delas: um público,
um regime de compatibilidade, um documento.

**Porquê e não um processo separado a chamar a API:** o isolamento que o pedido
pede não é «a org certa» — é «esta pessoa, esta capacidade, este recurso». Essa
decisão está em `org::decide` e na visibilidade de cada recurso. Um processo
separado teria de (a) inventar uma credencial que age por uma pessoa, e (b)
reimplementar a autorização ou confiar que a v1 a faz — e a v1 não expõe
legendas, capítulos nem Estúdio. Seria copiar uma regra em vez de a chamar, que
é a primeira das três regras da casa e tem catraca própria.

### 2. Transporte: Streamable HTTP, JSON-RPC 2.0, sem stdio

- `POST /api/mcp` — uma mensagem JSON-RPC por pedido; resposta `application/json`
  ou `text/event-stream` quando a tool emite progresso.
- `GET /api/mcp` — fluxo de eventos do servidor para notificações; sem sessão
  aberta responde `405`, que o protocolo permite.
- `DELETE /api/mcp` — termina a sessão MCP.
- `Mcp-Session-Id` identifica a sessão MCP. **O estado dela não vive na memória
  do nó**: o servidor corre com três réplicas (ADR-0001), e estado entre dois
  pedidos HTTP vive em Redis, como a presença e os lugares já fazem.
- Nada de stdio: uma ponte local obrigaria o segredo a viver no portátil e
  deixaria de fora quem usa o cliente na web.

### 3. Uma tool chama a mesma função que o handler HTTP chama

Regra de desenho, e é a que a revisão vai medir: **uma tool nunca lê a base de
dados nem chama o LLM directamente.** Chama a função de serviço que o handler do
BFF chama, e por isso herda a autorização, o DLP, os limites e a auditoria sem
os repetir. Uma tool que precise de lógica nova obriga a extrair essa lógica
para onde o handler também a chame — nunca a duplicá-la.

Consequência prática: `server/src/mcp/` é um **adaptador**, da mesma natureza
que `server/src/lib.rs` é para o BFF. Se um ficheiro em `mcp/` tiver uma query
SQL ou um `prompt`, está errado.

### 4. Não-invasão em três camadas, e nenhuma delas é o cliente

Um agente alcança **só** o que a pessoa alcançaria na web, e nunca mais:

| Camada | Quem decide | O que recusa |
|---|---|---|
| **Escopo do token** | OAuth (ADR-0028) | a tool não é anunciada nem aceita; `-32601` |
| **Capacidade da pessoa** | `org::decide` (ADR-0008) | `403` com o código de hoje |
| **Alcance do recurso** | a mesma `readable`/`library`/partilha do BFF | `404` — um id de outra org e um id inexistente dão a MESMA resposta |

E uma regra que fecha a porta mais fácil: **nenhuma tool aceita `org_id` nem
`user_id` do cliente.** A organização vem da pertença de quem autenticou. Um
parâmetro desses num esquema de tool é um defeito, e vai ter portão (§7).

### 5. `tools/list` anuncia só o que aquela pessoa pode fazer

O catálogo é **fechado** (como o `Scope::ALL` das chaves `dlx_`), mas a lista
devolvida é filtrada pelo cruzamento dos escopos do token com as capacidades da
pessoa. Anunciar uma tool que vai responder `403` é vender o que não se tem — a
segunda regra da casa, e o `check-capability-claims.sh` existe para isso.

### 6. Trabalho longo devolve um id, nunca bloqueia

Traduzir legendas, gerar capítulos, compor um recorte e publicar um *short* são
tarefas de minutos. Uma tool que as começa devolve `{job_id, status}` e há uma
tool de estado. O tecto de ffmpeg em simultâneo continua a ser o do `recorder`
(`fair_slots`, justiça por organização) e o do LLM continua a ser o semáforo por
org do `ai_studio` — um agente não é um caminho para furar nenhum dos dois.

### 7. O que isto obriga a provar (portões)

| Portão | O que mede |
|---|---|
| `check-route-auth.sh` | as rotas `/api/mcp` têm extractor próprio (`McpAuth`), não ficam públicas |
| `check-isolamento-cobertura.sh` + `isolamento.mjs` | cada tool com escopo de organização é exercitada com um token da **outra** org |
| **`check-mcp-tools.sh`** (novo) | toda a tool do catálogo: (a) está no documento OpenAPI/descritor commitado, (b) declara capacidade e escopo, (c) não tem `org_id`/`user_id` no esquema de entrada, (d) é exercitada pelo teste de isolamento |
| `cargo test --release` | um teste de duas organizações por família de tool (a regra de multi-tenant a 100%) |

### 8. Auditoria: um agente é tão rastreável como a pessoa

Toda a chamada de tool que escreve grava na trilha de auditoria o nome da tool,
o cliente MCP, o recurso e a pessoa por quem agiu. Sem isto, «foi o agente» é
uma resposta que ninguém pode verificar.

### 9. O que NÃO é uma tool

**Media.** MCP transporta JSON; áudio e vídeo continuam no SFU. A tool de
tradução ao vivo **configura** (liga, escolhe línguas, diz o estado); a pista
traduzida é produzida no caminho de media e é assunto de ADR próprio
(dobragem com voz sintetizada, decidida a 2026-10-13: entra, com TTS novo).
Uma tool que prometa «traduzir o áudio» e devolva texto seria o relato
desonesto que a casa recusa.

**Amostragem e elicitação** (`sampling`, `elicitation`) ficam declaradas como
ausentes no `initialize`: deixar um agente externo pedir ao nosso servidor que
chame um LLM é uma porta para gastar o modelo partilhado de outra organização.

## Alternativas recusadas

| Alternativa | Porque não |
|---|---|
| Processo separado com chave `dlx_` | a chave age por quem a criou, só um admin a emite, e os sete escopos não abrem nada do que o pedido quer (§Contexto 4) |
| Ponte local em stdio | o segredo passa a viver no portátil, e quem usa o cliente na web fica de fora |
| Tools a ler a base directamente | copiava a autorização; a catraca da arquitectura recusa-o, e a R41 mostra o que acontece quando um handler se esquece |
| `org_id` como parâmetro de tool | transforma o isolamento numa escolha do cliente |

## Consequências

- **Boas:** uma tool nova custa o esquema e a ligação ao serviço; a autorização
  vem de graça e é a mesma que o browser prova todos os dias; um token
  revogado mata o agente no mesmo instante em que mata a sessão (ADR-0028 §4).
- **Más:** a superfície MCP sobe a conta das rotas que os portões varrem, e o
  descritor de tools passa a ser contrato commitado — mais um ficheiro que pode
  derivar. O `check-mcp-tools.sh` existe para que derive em vermelho.
- **Por decidir:** se o catálogo de tools entra no SDK público do inquilino
  (hoje é só a v1). Fica fora até haver um segundo consumidor.
