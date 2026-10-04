# Delonix Meet — Análise Competitiva e Posicionamento

*Documento vivo. Atualizar sempre que lançarmos ou os concorrentes lançarem funcionalidades relevantes.*

---

## O que Zoom, Teams e Meet fazem de melhor

### Zoom — o melhor em...
- **Fiabilidade de chamada:** anos de engenharia em adaptação de rede (bitrate adaptation, packet recovery, FEC) tornam o Zoom funcional mesmo em redes ruins. O protocolo proprietário (não WebRTC puro) permite otimizações que o navegador não deixa fazer.
- **Breakout rooms:** inventaram e popularizaram o conceito no enterprise. A gestão é madura: distribuição automática ou manual, broadcast para todos os breakouts, timer visível, retorno automático.
- **Webinar mode:** modo assimétrico (host+painelistas vs. audiência passiva) com Q&A moderado, polling, raise hand. Zoom Events é uma plataforma de eventos separada.
- **Hardware ecosystem:** Zoom Rooms (hardware certificado), Zoom Phone (PBX cloud), Zoom Contact Center. Ecossistema integrado sem comparação.
- **SDK/API maturidade:** SDK nativo para iOS/Android/Desktop maduro; webhook API abrangente; Marketplace com centenas de apps.
- **UX de simplicidade:** entrar numa reunião Zoom como convidado (sem conta) ainda é o mais simples do mercado.

**Fraquezas do Zoom:**
- 100% cloud americana — dados saem do país
- Reputação de privacidade danificada (Zoom bombing 2020, acusações de envio de dados para China)
- Preço: add-ons custosos (PSTN, Zoom Phone, Zoom AI Companion)
- Electron pesado (≈500MB, usa mais CPU que o Meet no browser)
- Sem self-hosting real

### Microsoft Teams — o melhor em...
- **Integração Office 365:** reunião num clique do Outlook, notas no OneNote, ficheiros no SharePoint, gravação no Stream. Se a empresa já está na Microsoft 365, Teams é invisível de instalar.
- **Together mode:** modo de IA que coloca todos numa "sala virtual" com fundo partilhado — reduz fadiga de reuniões provado por estudos internos.
- **Compliance e eDiscovery:** retenção legal de mensagens e gravações, hold legal, auditoria para reguladores (financeiro, saúde, governo).
- **Canais + reuniões unificados:** reuniões nascem dentro de canais de equipa — contexto de projeto sempre presente.
- **Copilot for Teams:** notas automáticas, resumos, action items gerados por AI durante a reunião (Microsoft 365 Copilot, pago).
- **Teclado virtual + noise suppression:** supressão de ruído baseada em AI; transcrição automática.
- **PSTN integrado:** Teams Phone (PSTN via Microsoft ou operador certificado), calling plans, auto-attendant, call queues.

**Fraquezas do Teams:**
- **Só valioso no ecossistema Microsoft** — sem M365 é uma ferramenta desconfortável
- UX complexa (Teams tem mais botões que qualquer outro produto Microsoft combinado)
- Performance no browser piora com muitos participantes
- Reuniões fora de canais têm contexto zero
- Preço por seat alto; Copilot custa $30/user/mês extra

### Google Meet — o melhor em...
- **Simplicidade extrema:** UX mais limpa do mercado. Entrar = um clique. Sem instalação no browser.
- **Qualidade de vídeo em redes fracas:** Duo/Meet tem compressão adaptativa excelente; qualidade percebida > Zoom em redes móveis instáveis.
- **Companion mode:** entrar numa reunião em dois dispositivos simultaneamente (ex: ecrã grande + telefone para reações) sem eco.
- **Smart framing (hardware):** Google Meet Desk com câmara que enquadra automaticamente o orador.
- **Integração Google Workspace:** agenda, Drive, Gmail — se a empresa usa Google, Meet aparece em todo o lado.
- **Noise cancellation:** supressão de fundo (ventoinha, digitação) sem plugin.
- **Tile view adaptativa:** resolve quem mostrar em destaque automaticamente (speaker detection + face detection).

**Fraquezas do Meet:**
- **Funcionalidades básicas pagas** no plano Business (gravação, breakouts, whiteboards requerem upgrade)
- Sem PSTN no plano básico
- Whiteboard (Jamboard) foi descontinuado em 2024 → integração com FigJam/Miro/etc. como 3rd party
- Limitado fora do ecossistema Google
- Não há SDK público funcional (integração = iframe)

---

## O que nenhum dos três oferece (oportunidade do Delonix)

### 1. Self-hosting real sem royalty
Zoom, Teams e Meet são 100% SaaS. Não existe opção de self-host credível. Organizações com dados sensíveis (governos, bancos, saúde, defesa) precisam de uma solução que corra no seu datacenter, sob o seu controlo.

**Delonix:** binário único Rust + docker-compose + nginx. Deploy num VPS de €10/mês. Dados nunca saem do servidor do cliente.

### 2. Soberania de dados e conformidade local
Os três grandes processam dados em servidores americanos (sujeitos ao CLOUD Act, FISA 702). Para bancos em Angola (BNA), empresas sob LGPD no Brasil, ou qualquer entidade europeia sob GDPR rigoroso, isto é um bloqueio legal real.

**Delonix:** dados onde o cliente quiser. Air-gap deployable.

> **Não «conformidade BNA out-of-the-box».** O servidor não tem definição nenhuma de
> residência de dados — a conformidade vem de ONDE se instala, e é do instalador, não do
> produto. Dizer o contrário numa proposta é uma afirmação que o código não sustenta.

### 3. E2EE real em grupo com gravação
- Teams: E2EE apenas em chamadas 1:1
- Zoom: E2EE opcional (pago, quebra funcionalidades)
- Meet: E2EE em trânsito (TLS), não E2EE de ponta-a-ponta real

**Delonix:** E2EE via Insertable Streams AES-256-GCM, **opcional por sala**
(`rooms.e2ee: bool`, `server/src/rooms.rs:32`) — não «sempre activo»; gravação com key delegation (anfitrião cede chave explicitamente, servidor decifra só para gravar, chave nunca persiste).

### 4. Backend em Rust — sem GC, sem pausas
Os backends de Zoom/Teams/Meet incluem Java, Go e código C++ legado com GC. Pausas de GC causam jitter de áudio/vídeo em picos de carga.

**Delonix:** Rust — sem GC, sem garbage collection pauses, memória safe sem overhead, latência determinística. O SFU faz fan-out de RTP sem desencriptação num loop tight.

### 5. Hierarquia organizacional nativa
Zoom tem accounts/sub-accounts. Teams tem tenants/equipes. Meet tem Google Workspace domains.

**Delonix:** `organizations → branches → employee_groups → org_members` com roles, titles e salas presenciais por organização. Estrutura que espelha como empresas africanas e lusófonas realmente funcionam (sede + filiais regionais).

### 6. MoM por AI configurável e local
- Zoom AI Companion: $7/user/mês, dados vão para a cloud Zoom/OpenAI
- Copilot Teams: $30/user/mês, dados vão para a cloud Microsoft/OpenAI
- Duet Google: $30/user/mês, dados vão para a cloud Google

**Delonix:** MoM gerado **só pelo Ollama do cluster** (`server/src/ai.rs`). Não há
nenhuma chamada à Claude API nem a qualquer cloud externa em `server/src` — verificado a
2026-09-30. Sem `OLLAMA_URL` a IA fica desligada, não degrada para uma cloud. Para organizações com dados classificados: LLM completamente local, sem dados a sair do servidor.

### 7. Webhooks + API keys no core (não add-on)
Zoom e Teams têm marketplace de apps com webhooks, mas são complexos de configurar e têm rate limits agressivos nos planos base.

**Delonix:** `org_webhooks` com HMAC, suporte Slack/Teams/Mattermost/generic; API keys por org com scopes — incluídos no core, sem plano pago.

### 8. Sem vendor lock-in
Exportar dados do Zoom/Teams/Meet para outro sistema é difícil por design.

**Delonix:** PostgreSQL acessível, gravações em .webm standard, MoM em texto plain, API aberta. Migrar ou integrar é uma query SQL.

---

## Funcionalidades que o Delonix ainda não tem (honestidade)

| Funcionalidade | Zoom | Teams | Meet | Status Delonix |
|---|---|---|---|---|
| Entrada de convidado SEM conta | ✅ | ✅ | ✅ | **Meio feito — continua a ser o bloqueio nº 1 à adopção.** O servidor tem a rota pública `POST /api/rooms/{room_code}/guest-join` (`server/src/guests.rs`) e o cliente tem `guestJoin` (`web/src/api.ts`), mas **nenhum ecrã a chama**: quem não tem sessão é mandado para o Login (`web/src/App.tsx`). Medido a 2026-10-04; ver o plano de lacunas de 2026-10-04 (`docs/plano-lacunas-2026-10-04.md`), item E1 |
| PSTN dial-in | ✅ | ✅ | ✅ | **Existe** desde o #130: um telefone entra na reunião e ouve e é ouvido ([ADR-0010](adr/0010-ponte-telefone-sala.md), R221/R222), medido contra um FreeSWITCH real. Falta a cadeia com operadora e Kamailio. Os troncos, o plano de marcação e o CDR estão no servidor (#136), mas a configuração que o compose, o cluster e o chart distribuem **não os liga** — só `voice/freeswitch/telefonia-prova/` (plano de lacunas, T1) |
| App mobile nativa | ✅ | ✅ | ✅ | **Não existe neste repositório** (zero projectos nativos, medido a 2026-10-04). Há PWA, sem notificações com a aplicação fechada |
| SSO OIDC | ✅ | ✅ | ✅ | **Implementado** (`auth.rs`, rotas `/sso/discovery\|authorize\|callback`, tabela `org_sso_configs`) — não é stub |
| SAML | ✅ | ✅ | ✅ | Não existe |
| SCIM provisioning | ✅ | ✅ | ✅ | Roadmap |
| Marketplace/plugins | ✅ | ✅ | ⚠️ | Roadmap |
| Webinar mode | ✅ | ✅ | ⚠️ | Não planeado (Fase 7+) |
| Hardware rooms | ✅ | ✅ | ✅ | Não planeado |
| Live captions (API) | ✅ | ✅ | ✅ | Whisper local (feito) |
| MLS key agreement | ✅ (Zoom) | ⚠️ | ❌ | Roadmap. `server/src/mls.rs` é desenho, sem rota montada nem cliente — a E2EE é chave partilhada AES-GCM (`web/src/e2ee.ts`) |
| Remote desktop control | ✅ | ✅ | ❌ | **Recusado até haver agente nativo** — o handshake de sinalização existe e está testado; o encaminhamento de input não existe e NÃO PODE existir só no browser (ver `web/src/capabilities.ts` e R109) |
| DLP / information protection | ❌ (⚠️Teams) | ✅ | ❌ | **Existe, estreito** (`server/src/dlp.rs`, ligado ao chat, às actas, às transcrições e às gravações): três expressões fixas (cartão, NIF, chave `sk-…`) e máscara de palavrões. Sem política por organização, sem registo de ocorrências, sem ficheiros nem ecrã partilhado |

---

## Posicionamento por segmento

| Segmento | Recomendação atual | Porquê Delonix ganha |
|---|---|---|
| PME angolana / lusófona | **Delonix self-host** | Custo zero por seat, dados locais, PT nativo |
| Banco / governo / defesa | **Delonix air-gap** | Único com self-host real + E2EE + auditoria |
| Startup tech SaaS | **Delonix SaaS** | Funcionalidades parity, preço, open API |
| Enterprise Microsoft | Teams (por agora) | Integração M365 ainda não replicada |
| Enterprise Google | Meet (por agora) | Integração Workspace ainda não replicada |
| Ensino superior | **Delonix self-host** | Budget limitado, dados de estudantes sensíveis |
| ONG / setor social | **Delonix self-host** | Custo zero, privacidade beneficiários |

---

## Inspiração de UX a adoptar

### De Zoom
- ✅ Gestão de breakout rooms (distribuição + timer + broadcast) — **feito**
- Modo webinar assimétrico — roadmap
- Zoom Apps sidebar (apps in-meeting) — roadmap SDK
- Galeria de reações (emoji picker completo)

### De Teams
- ✅ Together mode (fundo partilhado) — parcialmente via RVM matting
- ✅ Noise suppression — integrado via media.ts
- Canais de equipa persistentes com histórico de reuniões — roadmap
- ✅ Compliance/retenção configurável — feito (retention_days + sweep)
- Live component (colaboração em tempo real em documento durante reunião)

### De Meet
- ✅ Grid layout adaptativo (best-fit 16:9) — feito
- ✅ Companion mode (participar em dois dispositivos) — **feito de verdade desde R114**: o
  servidor reconhece a segunda sessão da MESMA conta e ela entra sem microfone e sem som,
  com o aviso a explicar porquê e um botão para trazer o áudio para aqui. Antes disto a
  linha dizia «via QoS + múltiplos joins», que era outra maneira de dizer «entrar duas
  vezes funciona» — e funcionava, com um ciclo de eco.
- ✅ Tile view com speaker detection — feito
- ✅ Controles estilo pill dividida mic/câmara — feito
- Smart framing por software (face detection → crop automático) — roadmap

### O que nenhum tem e Delonix pode liderar
- **Hierarquia de org com chamadas estilo WhatsApp** entre colleagues — feito
- **MoM com tarefas parseadas** (checkbox na ata) — feito
- **Whiteboard que sobrevive à sessão** (persistido, reaberto) — feito
- **E2EE verificável** (código de segurança SHA-256 em 4×5 dígitos) — feito
- **Deploy num comando** com todas as dependências incluídas — feito
- **Temas corporativos** (NgolaCloud, Kaeso) sem rebrand pago — feito
