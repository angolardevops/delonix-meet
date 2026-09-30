# Delonix Meet: o que pode levar alguém a trocar o Meet, o Teams ou o Zoom

> **Resgatado a 2026-09-30 de uma pasta temporária, onde esteve catorze dias.**
> Análise feita a **2026-09-16** por leitura de código, contra `origin/main` `4ff5249`.
> Entra no repositório por duas razões: apanhou quatro afirmações de venda que o código
> desmente (§0), e nomeia o bloqueio de adopção número 1 — **que continua verdade hoje**.
>
> **O bloqueio confirmado a 2026-09-30 sobre a `main` `9113b79`:** `join_room` continua a
> exigir `AuthUser` (`server/src/rooms.rs:480`). Existe um papel `guest`, mas é convidado
> *da agenda da organização* e também precisa de conta. **Não há entrada anónima por
> link.** É a alavanca 1 e o primeiro item do §3, e nada no repositório o registava.
>
> **O que já não é verdade no texto abaixo** — corrigido aqui, não lá, para se ver o que
> se andou:
>
> | Onde | O que dizia | O que é verdade a 2026-09-30 |
> |---|---|---|
> | Alavanca 2, bloqueio 6 | «fechar as falhas S4–S6» | **fechadas**: S4 (SSRF) na R180, S5 (segredos em claro) na R160, S6 (chaves sem escopos) nas R170/R171 |
> | Alavanca 10, bloqueio 8 | «a ponte FreeSWITCH↔SFU (`sfu.rs:611` é um stub)» | **a ponte existe** desde o #130 ([ADR-0010](adr/0010-ponte-telefone-sala.md), R221/R222): um telefone entra na reunião e ouve e é ouvido, medido contra um FreeSWITCH real. Falta a cadeia com operadora e Kamailio |
> | Bloqueio 10 | «não há OpenAPI nem gRPC» | **há os dois**: specs geradas e commitadas em `docs/reference/openapi/`, catraca a zero; gRPC interno com mTLS |
> | Bloqueio 11 | «o ramo de UI está afastado de `main`» | **resolvido**: a consola nova entrou nos #119/#120 e o Estúdio-TV no #129 |
> | §0 | «`gap-sessao.md` diz que o chat nunca é gravado» | já estava corrigido no próprio texto; confirma-se — `room_chat.rs` faz o `INSERT` |
>
> O resto — os segmentos, as dez alavancas, o plano a 90 dias e o «o que não está
> validado» — **não foi reverificado**. Lê-o como uma análise datada, não como o estado
> de hoje.

---


*Análise feita só com leitura, no worktree `ui-template-rebuild` (commit `3153fee`), com uma verificação a `origin/main` (commit `4ff5249`), a 2026-09-16. Os caminhos são relativos a `/home/walter/workspace/ngolacloud/.worktrees/delonix-meet/ui-template-rebuild/`, salvo indicação em contrário.*

## Resposta curta

Hoje ninguém deixa o Google Meet ou o Teams por o Delonix ter mais funções. Troca quem **não pode** usar esses produtos: a lei não deixa os dados saírem de Angola, o preço por lugar em USD pesa numa instituição paga em AOA, o ERP é o Odoo, ou a pessoa ensina e emite e precisa de gravar, editar, legendar e fazer directo num só sítio.

Três destas vantagens já existem no código e estão medidas: a soberania (self-host, auditoria encadeada, IA local), o Odoo e o Estúdio.

Há um defeito que bloqueia a troca antes de qualquer outra coisa. **Um convidado externo não consegue entrar numa reunião sem ter conta.** O `join_room` exige `AuthUser` (`server/src/rooms.rs:280-284`) e o próprio ecrã avisa que não há entrada anónima (`web/src/pages/auth/EntrarComCodigo.tsx:2`). O Zoom e o Meet ganham exactamente aqui.

## 0. Verificações que mudam a leitura

**Os documentos de venda estão desactualizados em relação ao código:**

| O que se diz | Onde se diz | O que o código mostra |
|---|---|---|
| «E2EE sempre activo» | `docs/competitive-positioning.md` §3 | A E2EE é opcional em cada sala: `rooms.e2ee: bool` (`server/src/rooms.rs:29,114`). O README (§6) diz «opcional». |
| Atas geradas pela Claude API ou pelo Ollama | `competitive-positioning.md` §6 | Não há chamadas a Anthropic ou Claude em `server/src`. Só existe o Ollama (`server/src/ai.rs`). |
| «SSO: stub» | `competitive-positioning.md`, tabela de lacunas | O OIDC está implementado: `auth.rs:590-800`, rotas `/sso/check|login|callback` (`main.rs:150-152`) e tabela `org_sso_configs`. Não há SAML nem SCIM. |
| «Conformidade BNA out-of-the-box» | positioning e `HARNESS.md` §9 | O servidor não tem nenhuma definição de residência. Depende do sítio onde se instala (`gap-entrada-gestao.md`, linhas marcadas EXTERNAL). |

**Algumas notas de lacunas já não correspondem ao código:**
- `gap-sessao.md` diz que o chat nunca é gravado. Hoje é gravado em `server/src/room_chat.rs:111`.
- A sonda de rede existe (`net_probe.rs`).
- As legendas, os capítulos, os metadados das gravações e os destinos de emissão guardados também existem (migrações 0040–0047).

**Estado dos ramos.** Este ramo está 72 commits à frente de `origin/main` e 11 atrás. O gateway de SMS só está em `main` (`server/src/sms.rs`, `sms_smpp.rs`, `sms-gateway/`, migração `0039_sms_gateway.sql`). Há colisões por resolver entre os dois ADR-0005 e a numeração das migrações (`notas-ui-template/PLANO-REBASE-ADR0004.md`).

## 1. Segmentos onde o Delonix pode ganhar

### A. Sector público angolano e organizações reguladas (BNA, saúde, operadores)
- **O que precisam:** reuniões internas e com entidades externas sem que áudio, gravações ou atas saiam do país, e com uma auditoria que resista a um inspector.
- **Onde os incumbentes falham (factual; os preços e regiões são de conhecimento público e devem ser confirmados antes de ir para uma proposta):**
  - Meet, Teams e Zoom são SaaS em datacenters fora de Angola. As regiões mais próximas são na África do Sul ou na Europa.
  - A Lei 22/11 (protecção de dados) exige base legal ou autorização da APD para transferências internacionais.
  - O Teams e o Meet não têm self-host. O Zoom tem conectores híbridos que mantêm a media on-prem, mas o plano de controlo continua na cloud.
  - A IA dos três (Copilot, Gemini, AI Companion) corre na cloud do fornecedor.
- **O que o Delonix já tem, medido:**
  - Instalação em binário único, docker-compose ou K8s (`deploy/`).
  - Auditoria imutável com cadeia de hashes e verificação em `/api/orgs/{id}/audit/verify` (`server/src/audit.rs`, migração 0037).
  - Retenção configurável e MFA TOTP (`mfa.rs`).
  - Isolamento entre organizações testado (`web/e2e/isolamento.mjs`) e RLS como defesa extra (ADR-0002, só em parte das tabelas).
  - IA que corre só no cluster: Ollama (`ai.rs`), faster-whisper (`whisper-server/app.py`, `ai-worker/transcribe_worker.py`) e Whisper no browser (`web/src/whisperWorker.ts`).
- **Atenção:** as transcrições ao vivo usam a Web Speech no Chrome, que **envia o áudio à Google** (`web/src/media.ts:539-552`, `HARNESS.md` §11). Para este segmento tem de ficar desligada por política.

### B. Universidades, centros de formação e escolas de negócio
- **O que precisam:** dar aulas síncronas com participação activa, gravá-las, legendá-las e publicá-las, com um orçamento baixo e estudantes em 3G/4G.
- **Onde os incumbentes falham:**
  - O Meet exige planos Education pagos para gravação e salas de grupo.
  - O Zoom cobra por anfitrião e cobra à parte os webinars.
  - Nenhum edita nem legenda de forma séria: o fluxo real é Zoom + OBS + Descript.
- **O que o Delonix já tem:**
  - Sondagens e **quiz** com resposta correcta (`web/src/room/PollsPanel.tsx:12,87`), perguntas e respostas com votos, temporizador (`useMeetingTools.ts`).
  - Salas de grupo no formato `training` (`rooms.rs:30,64`; `BreakoutsCard.tsx`).
  - Quadro branco que se guarda (`whiteboards.rs`).
  - Editor UML/BPMN com validação (`web/src/pages/diagrams/validate.ts`, `examples.ts`).
  - O Estúdio (segmento E).

### C. Empresas que já usam Odoo (carteira Kaeso/NgolaCloud)
- **O que precisam:** marcar e entrar em reuniões a partir do ERP, com os utilizadores sincronizados e a ata a voltar ao registo.
- **Onde os incumbentes falham:** o Teams e o Meet só se integram com M365 e Workspace. Com o Odoo, há conectores de terceiros.
- **O que o Delonix já tem:**
  - Login com a conta Odoo, que cria a organização a partir da empresa e sincroniza os utilizadores internos (`server/src/odoo_sso.rs`).
  - API de reuniões idempotente por `external_ref` (`meetings_v1.rs`).
  - Provisionamento de utilizadores (`odoo.rs`) e calendário Odoo na Home (`web/src/pages/home/useOdooCalendar.ts`).
  - O módulo `nk_delonix_meet` vive fora deste repo (`docs/nk-delonix-meet-integration.md`).

### D. Organizações com rede fraca: províncias, ONG, delegações
- **O que precisam:** que a chamada continue a funcionar com perda de pacotes e pouca banda, e poder entrar só com áudio.
- **Onde os incumbentes falham:** o Meet é forte aqui e não se deve dizer o contrário. A falha dos incumbentes está no custo dos dados móveis e na ausência de números de marcação angolanos incluídos.
- **O que o Delonix já tem:**
  - Uma política de camadas que ajusta ao tamanho do tile, à perda, ao RTT, à bateria e ao `saveData` (`web/src/layerPolicy.ts:40-120`).
  - O servidor só reencaminha os 3 microfones mais activos (R22).
  - Entrada só com áudio (`Prejoin.tsx:67`, `Room.tsx:188-190`) e sonda de rede antes de entrar (`net_probe.rs`).
  - Uma matriz `tc netem` real, incluindo 3G e 20% de perda (`web/e2e/netem-matrix.mjs:39-49`). **Não corre no CI** e não há resultados publicados.

### E. Formadores, igrejas e media que emitem em directo
- **O que precisam:** gravar, cortar, legendar e emitir para vários destinos sem juntar 3 ou 4 ferramentas.
- **O que o Delonix já tem:**
  - Palco com cenas, faders e sobreposições, até 2160p50 (`web/src/studio/palco.ts:71-90`).
  - Editor não destrutivo com linha de tempo, mistura, IndexedDB e exportação com frames exactos (`studio/edit/*`, `studio/exports/*`).
  - Legendas no browser (`studio/captions/transcricaoWorker.ts`).
  - O Estúdio funciona offline (`web/public/sw.js`, `studio/offline.invariantes.test.ts`).
  - Directo RTMP com um ffmpeg por destino e supervisor (`server/src/broadcast.rs`, ADR-0003).
  - Legendas WebVTT e capítulos automáticos nas gravações (`recording_captions.rs`, `recording_chapters.rs`).

## 2. As 10 alavancas de adopção

Ordenadas por impacto na troca multiplicado pela viabilidade, dado o código de hoje. Esforço: S (pequeno), M (médio), L (grande).

| # | Alavanca | O que existe hoje | O que falta | Esforço | Métrica de prova |
|---|---|---|---|---|---|
| 1 | **Entrar como convidado sem conta**, só com link e nome | O token de sala já leva a origem `guest` (`rooms.rs:318-322`, `auth.rs:48`); sala de espera e admissão (`signaling.rs`) | Uma rota pública `POST /api/rooms/{code}/guest-join` que emita um token de sala efémero, com limite de pedidos, sempre pela sala de espera e sem acesso a gravações | S–M | % de convidados externos que entram em menos de 60 s sem ajuda (objectivo: ≥90%) |
| 2 | **Pacote de soberania (Lei 22/11)**: on-prem em Luanda, IA só local, auditoria verificável | `audit.rs` com verificação, retenção, `mfa.rs`, `ai.rs`, `whisper-server`, `deploy/k8s` | Política ao nível da org para «nunca Web Speech» e «IA só local»; uma página de residência com dados do servidor e não com texto fixo (hoje EXTERNAL); fechar as falhas S4–S6 (SSRF no OIDC/WebDAV, segredos em claro, chaves de API sem escopos: `docs/auditoria-2026-09-16-backend.md:50-52`); corrigir a E2EE «sempre activa» no texto de venda | M | Uma DPIA ou parecer jurídico aceite por um cliente piloto; zero pedidos de saída para fora do cluster numa captura de rede durante uma reunião com legendas e ata |
| 3 | **Preço por servidor ou instituição, não por lugar**, facturado em AOA | Não há código de facturação, lugares nem plano (o Admin não tem as colunas LUGARES e PLANO, `gap-entrada-gestao.md:236-239`). O `check-capability-claims.sh` impede vender o que não está construído | É sobretudo decisão comercial. Em código, só um limite de lugares por org, se se quiser escalões | S | Custo total a 3 anos para 500 utilizadores comparado com M365 Business Basic e Workspace Business Standard (preços públicos em USD, a confirmar) |
| 4 | **Integração nativa com o Odoo** | `odoo_sso.rs`, `meetings_v1.rs`, `odoo.rs`, `useOdooCalendar.ts`, `pages/integrations/OdooCard.tsx` | Sincronizar contactos `res.partner` (hoje só `res.users`, `odoo_sso.rs:176`); expor a origem Odoo e o estado de sincronização nas reuniões (`meeting_external_refs` não sai em `meetings::list`); `GET /api/v1/meetings?since`, notas e o webhook `meeting.mom_ready` ainda por fazer (`docs/nk-delonix-meet-integration.md`) | M | Percentagem das reuniões do piloto criadas no Odoo; atas que voltam ao `strategic.meeting` em menos de 10 min |
| 5 | **IA local em português**: atas, legendas, capítulos e tradução | Atas e tradução via Ollama para pt/en/fr/es/de/zh (`ai.rs:47-70`); capítulos e WebVTT; Whisper large-v3 em lote; Whisper-tiny no browser | Medir a qualidade em pt-AO (WER) e guardar a confiança média, que hoje se deita fora (`transcribe_worker.py`); registar o uso da IA na auditoria (`ai.rs` nunca chama `audit::log`); página de políticas de IA (não existe). **Umbundu, Kimbundu e Kikongo ficam de fora de propósito** (`ai.rs:47-51`) e são trabalho de investigação (L) | M | WER em pt-AO abaixo de um limiar acordado num corpus de 5 h do cliente; percentagem de atas aceites sem edição |
| 6 | **Convites e lembretes por SMS**, com email a seguir | O gateway de SMS está em `origin/main`: fila, encaminhamento USB/SMPP, idempotência e testes e2e (ADR-0005). **Não há SMTP nem email** em `server/` | Juntar os eventos das reuniões (convite, lembrete, alteração de hora) ao envio de SMS; confirmar os prefixos dos operadores junto do regulador (ADR-0005 marca-os A CONFIRMAR); contrato SMPP com um operador. O Samsung medido **não envia** por USB (só MTP). Faltam também SMTP e recuperação de palavra-passe | M | Taxa de presença nas reuniões com lembrete SMS comparada com as sem lembrete |
| 7 | **Sala de aula completa** | Quiz e sondagens, perguntas e respostas, salas de grupo, temporizador, quadro branco guardado, editor UML/BPMN, formato `training` | Relatório de presença e resultados do quiz por aluno (há `room_participants`, mas nenhuma rota os lista); o quadro só guarda traços, sem texto, notas ou páginas (`gap-sessao.md` W8–W16; confirmar depois do lote 1); exportação para LMS (Moodle) | M | Uma disciplina inteira (um semestre ou módulo) dada só em Delonix; percentagem de aulas com quiz |
| 8 | **Rede fraca comprovada** | `layerPolicy.ts`, os 3 oradores mais activos, entrada só com áudio, `net_probe.rs`, `netem-matrix.mjs` | Correr e **publicar** a matriz (congelamentos, perda de áudio, tempo até à primeira imagem); um modo «poupar dados» visível para o utilizador (a política aceita `preference: 'data-saver'`; falta confirmar se há ecrã); limitar a própria emissão. A PWA só funciona offline no Estúdio, e é assim de propósito (`sw.js`) | M | Áudio inteligível com 20% de perda e vídeo sem congelar em «3G-típico», contra o Meet no mesmo netem |
| 9 | **Estúdio que substitui Zoom + OBS + Descript** | Palco até 4K, editor, legendas, exportações, directo multi-destino com estado por destino (`broadcast.rs`, R123), destinos guardados e cifrados (migração 0047) | Directo **sem sair para o YouTube** (HLS servido pelo próprio servidor, para eventos soberanos); guardar gravações em S3/MinIO (`storage.rs` só tem local/NFS/WebDAV) | M | Horas por semana poupadas por um formador (entrevista antes e depois); emissões sem falhas |
| 10 | **Marcação telefónica com números angolanos** | Plano de controlo: DIDs, PIN, CDR, facturação (`voice.rs`, `main.rs:357-366`); IVR Lua + Kamailio + FreeSWITCH com SRTP obrigatório (`voice/`) | **A ponte FreeSWITCH↔SFU** (`sfu.rs:611` é um stub, `voice/README.md` «sub-fase 2b»): hoje quem liga só fala com outros que também ligaram. Faltam o contrato do SIP trunk (`docs/voice-rfi-sip-trunk.md`), chamadas de saída e DTMF | L | Um telefone fixo ou móvel entra numa reunião WebRTC e ouve e é ouvido, com MOS ≥3,5 |

**Fora do top 10:**
- **Ferramentas de migração** (impacto médio, M). Não há importação de Google Calendar ou Outlook e só existe exportação `.ics` por reunião (`meetings.rs:780-836`). Um importador `.ics`/CalDAV chega para um piloto.
- **Nitidez de imagem.** Não existe: só há `contentHint = 'detail'` na partilha de ecrã (`room/useScreenShare.ts:113`, `usePrejoin.ts:232`). Não vale a pena antes do resto.
- **Efeito «4D»/paralaxe.** O `HeadTracker` existe (`web/src/media.ts:474`, MediaPipe BlazeFace) e é aplicado à grelha e ao palco (`room/Stage.tsx:179-253`). Mas é **só local**: move a vista de quem está a ver. Dá uma boa demonstração e muda pouco a decisão de compra; não o poria na proposta a uma universidade.

## 3. O que impede a troca hoje

1. **Convidados sem conta.** Ver alavanca 1. É o bloqueio número 1 para reuniões com entidades externas.
2. **Não há email nem notificações para fora da app.** Não existe SMTP em `server/`: sem convites por email, lembretes ou recuperação de palavra-passe (`gap-entrada-gestao.md:39`). O SMS só está em `main` e ainda não está ligado às reuniões.
3. **Escala e fiabilidade não medidas em carga.**
   - O guia operacional dimensiona até cerca de 100 pessoas em 15–20 salas num só host (`docs/ops/platform-engineering.md:84-90`).
   - O SFU vive em memória por pod, com afinidade por sala (ADR-0001).
   - Em K8s, a alocação TURN está instável (`438 Stale nonce`, `HARNESS.md` §11).
   - Não há teste de carga com dezenas de participantes numa sala: os e2e usam 2 browsers e a matriz netem não corre no CI.
4. **Não há app móvel nativa.** Só PWA: não há pasta Flutter nem projecto iOS/Android no repo, logo não há push nem CallKit. O `HARNESS.md` diz «Flutter em progresso», mas isso não está neste repo.
5. **Não há integração com Google Calendar ou Outlook** (nem sincronização, nem add-in).
6. **Identidade empresarial.** O OIDC existe, mas o `client_secret` está em claro (S5). Não há SAML nem SCIM. As chaves de API não têm escopos nem expiração (S6).
7. **Armazenamento de gravações.** Não há S3/MinIO. Em multi-nó é preciso RWX, NFS ou object storage (`HARNESS.md` §11).
8. **Marcação telefónica** sem ponte de media nem trunk (alavanca 10).
9. **Webinar para grande audiência dentro da plataforma.** O formato `broadcast` existe, mas o público assiste no YouTube e afins, o que contradiz a soberania.
10. **Dívida de arquitectura que atrasa o SDK e o mobile.** Não há OpenAPI nem gRPC, há um ciclo de 18 módulos e o ADR-0004 continua «Proposto».
11. **O ramo de UI está afastado de `main`** e há rebase pendente com os ramos `backend-enterprise`, g4, g7 e g8 (`PLANO-REBASE-ADR0004.md`). Isto atrasa entregar tudo o que está acima num só build.

## 4. Plano a 90 dias para converter um piloto

**Pilotos sugeridos:** uma universidade ou instituto público angolano (segmento B com as exigências de A), mais um cliente Odoo da Kaeso (segmento C). Uma instalação on-prem em Luanda para cada um.

### Fase 1 (dias 0–30): «dá para usar com gente de fora e é legal»
- Juntar os ramos: rebase do lote 1 sobre `origin/main` (SMS) e dos ramos B, e resolver a colisão dos ADR-0005 e das migrações.
- Entrada de convidados sem conta (alavanca 1).
- Política da org «IA e transcrição só locais» (desliga a Web Speech) e fecho das falhas S4 e S5.
- Corrigir os documentos de venda: E2EE opcional, OIDC existe, não há Claude API.
- Instalar no piloto com TURN estável e correr a matriz netem na rede real do cliente.

**Métrica:** 20 reuniões reais com pelo menos 1 externo, ≥90% de entradas de convidados sem ajuda, zero pedidos de saída para fora do cluster e um parecer de conformidade Lei 22/11 do jurídico do cliente.

### Fase 2 (dias 31–60): «substitui o que usavam»
- Lembretes e convites por SMS ligados às reuniões (SMPP com um operador, ou USB com uma pen 3G/4G, que expõe AT) e SMTP mínimo.
- Odoo: `res.partner`, estado de sincronização e ata de volta ao Odoo.
- Sala de aula: relatório de presença e de resultados do quiz.
- Gravações em MinIO/S3.
- Publicar os resultados de rede fraca comparados com o Meet.

**Métrica:** ≥60% das reuniões do piloto marcadas dentro do Delonix ou do Odoo (e não no Meet, Teams ou Zoom), aumento da presença com lembrete SMS e ≥70% das atas aceites sem edição.

### Fase 3 (dias 61–90): «faz o que eles não fazem»
- Estúdio em produção numa disciplina ou evento: gravar, editar, legendar em PT e publicar, mais HLS próprio para emissão soberana.
- Teste de carga à maior sala do cliente (por exemplo 50–150 participantes numa aula), com SLO definido.
- Decidir a ponte PSTN (opção A `mod_verto` ou B, ponte RTP) e assinar o RFI do trunk. A entrega fica para depois dos 90 dias.

**Métrica:** 1 disciplina ou evento público inteiro feito só em Delonix, a sala de carga cumpre o SLO (áudio sem cortes com ≥95% dos participantes), e contrato ou renovação assinado com preço por instituição.

## O que não está validado

- Os preços e as regiões dos concorrentes são conhecimento público e mudam. Confirmar antes de uma proposta.
- A leitura jurídica da Lei 22/11 precisa de parecer.
- A qualidade do Whisper e do Ollama em pt-AO não foi medida.
- O comportamento em rede fraca não foi medido contra o Meet.
- Não sei se o modo «poupar dados» tem ecrã visível para o utilizador.
- O estado do quadro branco depois do lote 1 foi lido nas notas, não verificado no código.