# Fila de paridade — delonix-meet

Uma lacuna, um ficheiro, um contrato de sessão. Medido a **2026-10-18** contra `develop` @ `f21e6fc47`, e **remedido a 2026-10-21** contra `develop` @ `a16ab417b` (a árvore da `main` @ `7b9ff9c4` é igual).
A tabela completa com veredictos e evidência: [`docs/paridade-medida-2026-10-18.md`](../../docs/paridade-medida-2026-10-18.md). O que mudou desde então, id a id, com comando e código de saída: [`docs/paridade-medida-2026-10-21.md`](../../docs/paridade-medida-2026-10-21.md). **Antes de citar uma linha do de 10-18, vê a secção 4 do de 10-21**: cinco afirmações dele deixaram de ser verdade ou nunca o foram.

## Prompts com âncoras deslizadas — reler antes de correr

Medido a 2026-10-21, cada `ficheiro:linha` relido contra `a16ab417b`. Um prompt desta lista **não se corre tal e qual**: a sessão que o pegar relê as linhas primeiro (ou corre `/paridade novo` por cima). O detalhe linha a linha está na secção 3 do documento de 10-21.

| Prompt | Estado das âncoras |
|---|---|
| `A-01` | deslizadas (`signaling.rs:2346`→2420, `recorder.rs:1750`→2179, `voice.rs:1208` reescrita). Esperado: está fechado, e lê-se como histórico |
| `A-02` | âncoras de pé, mas **a medição mudou**: `recording_tracks` existe (0124–0126) e as duas rotas também. Fechado a 2026-10-21; as âncoras só importam como histórico |
| `A-04` | deslizadas (`01-config.yaml:23,27`→25,29; `signaling.rs:4130`→4269, `:4225`→4490) |
| `A-08` | deslizadas (`recorder.rs:2486`→2929; `lib.rs:2179,2248,2337`→2239, 2315, 2404) |
| `A-09` | deslizadas (`signaling.rs:342`→361, `:417`→436, `:1473`→1512, `:1738`→1785) |
| `B-06` | **errada desde o nascimento**: `sfu.rs:1689-1703` nunca foi o `reevaluate_room`. Está em `sfu.rs:1822-1837`, com o corpo igual ao de 10-18 |
| `B-10` | deslizadas (`lib.rs:456`→477, `.nest` `:599`→620) **e erradas desde o nascimento**: `scim.rs:64`→`:60` (`MAX_RESULTS`), `:344`→`:346` (`ListParams`), `:148`→`:54` (`dlxs_`) |

De pé: `A-03` e `A-05`.

**Como se usa.** `/paridade <id>` imprime o prompt tal e qual, com as recusas. A sessão que o corre abre o seu próprio contrato com `/escopo`. Um id sem ficheiro **não é recusa** — é o que o `/paridade novo <id>` existe para escrever.

**O que não se faz aqui:** implementar. O prompt escreve-se nesta fila; a correcção acontece noutra sessão, com contrato próprio.

## Grupo A — bloqueia uma venda ou é afirmação falsa

| id | Lacuna | Estado do prompt |
|---|---|---|
| `A-01` | [Um telefone entra numa sala cifrada de ponta a ponta](A-01-telefone-entra-em-sala-e2ee.md) | ✅ **FECHADO** 2026-10-18 — nos dois pontos de resolução da sala, com controlo negativo medido. Na `develop` desde o #378 (remedido a 2026-10-21: `grep -c e2ee voice.rs` = 9). Falta a prova por chamada real (sem DID de teste nesta máquina) |
| `A-02` | [Gravação ISO ligada por omissão e inexistente](A-02-gravacao-iso-ligada-e-inexistente.md) | ✅ **FECHADO** 2026-10-21: as pistas aparecem no leitor (visto no browser) e duas fontes dão dois ficheiros pela rota, com controlo negativo. Falta a prova ponta-a-ponta pelo SFU (não há e2e de estúdio a gravar) |
| `A-03` | [Ninguém é avisado sem ter a aba aberta](A-03-push-com-a-app-fechada.md) | ✅ **IMPLEMENTADO** 2026-10-21: Web Push no `ring`, com `offline` a passar a `ringing` e o 410 a limpar a subscrição, provado com controlo negativo. **Falta ver a notificação num aparelho real**, e o ADR-0032 está à espera do dono |
| `A-04` | [Quarenta pessoas por sala](A-04-quarenta-pessoas-por-sala.md) | ✅ **MEDIDO** 2026-10-21, numa máquina partilhada e declarada: sem simulcast reprova aos 48 por CPU; **com simulcast reprova aos 40 por subscrições em falta** (`B-12`). Recomendação: vender 40 e corrigir o `B-12` antes de qualquer decisão de arquitectura |
| `A-05` | [Calendário nos dois sentidos](A-05-calendario-nos-dois-sentidos.md) | escrito |
| `A-06` | Canais persistentes de equipa | por escrever |
| `A-07` | `require_mfa` não impõe MFA, e o lockout é por processo | por escrever |
| `A-08` | [Legal hold e eDiscovery](A-08-legal-hold-e-ediscovery.md) | escrito |
| `A-09` | [Modo webinar — dois formatos inertes](A-09-modo-webinar.md) | escrito |
| `A-10` | Certificações (SOC 2, ISO 27001, HIPAA) | **não é código** — é processo, e não entra nesta fila |
| `A-11` | SAML 2.0 | por escrever |

## Grupo B — atraso que se sente a usar

| id | Lacuna | Estado do prompt |
|---|---|---|
| `B-01` | Cliente móvel de reuniões (sem `flutter_webrtc`, motor SIP sem licença) | por escrever — **ler o `A-03` primeiro**: sem push, o móvel não resolve nada |
| `B-02` | Assistente de IA ao vivo na reunião | por escrever |
| `B-03` | Pesquisa semântica (hoje é lexical) | por escrever |
| `B-04` | Partilha de ecrã a 5–15 fps | por escrever |
| `B-05` | A composição rouba CPU ao media no mesmo pod | por escrever |
| `B-06` | [A entrada na sala é O(n) em série](B-06-entrada-na-sala-em-serie.md) | ✅ **FECHADO** 2026-10-21 **sem mudar o SFU**. O «1,7 s» era o espaçamento do gerador, e um A/B mostrou que tornar o laço concorrente não ajuda. Primeiro vídeo plano até 16; acima disso acompanha a CPU, o que passa para o `A-04` |
| `B-07` | API pública v1 é 3 % da BFF, e não há SDK | por escrever — remedido a 2026-10-21: v1 9/12, BFF 285/407 (era 282/404) |
| `B-08` | A sala de espera do convidado é obrigatória e sem interruptor | por escrever |
| `B-09` | Acessibilidade sem prova automática (`grep axe` = 0) | por escrever |
| `B-10` | [O SCIM trunca em silêncio acima de 200](B-10-scim-trunca-em-silencio.md) | ✅ **FECHADO** 2026-10-21: paginação RFC 7644 e `totalResults` real, provados contra Postgres com controlo negativo. Falta um ciclo com um provedor real (sem tenant de Entra ID/Okta). `/Groups` fica de fora, por decisão escrita |
| `B-12` | [Com simulcast, uma sala de 40 perde 8–15 % das subscrições de vídeo](B-12-subscricoes-em-falta-com-simulcast.md) | escrito 2026-10-21 — **o primeiro limite real da sala**, medido no `A-04`; causa por provar (hipótese: o recurso do `switch_layer`) |
| `B-11` | App de desktop | **defensável não ter** — ver o documento; não entra na fila sem decisão de produto |

## Grupo C — telefonia

| id | Lacuna | Estado do prompt |
|---|---|---|
| `C-01` | PBX: filas, voicemail, atendedor, supervisão, transferência | por escrever — é o maior, e precisa de decisão de produto antes do prompt |
| `C-02` | Softphone no nosso próprio produto | por escrever |
| `C-03` | Bordo de operadora (identidade, tectos de gasto, registo de entrada) | por escrever |
| `C-04` | Salas de hardware / SIP de vídeo | por escrever — **o bloqueio não é o codec**, é não haver gateway de vídeo; ver o documento |

## Porque é que 15 estão por escrever

Não por esquecimento. Um prompt que ninguém corre neste mês apodrece como qualquer outro documento — as citações `ficheiro:linha` deslizam e a próxima sessão herda uma fotografia errada. Escreveram-se os **9** que bloqueiam uma venda ou são afirmação falsa latente; os restantes **15** escrevem-se com `/paridade novo <id>` no momento em que alguém os for fazer, com as linhas relidas nesse dia.

## Ordem sugerida, se a pergunta for «por onde começo»

1. ~~`A-01`~~ e ~~`A-02`~~ (fechados a 2026-10-18 e 2026-10-21) — a gravação ISO que se prometia sozinha passou a existir e a aparecer no leitor.
2. `B-06` e `B-10` — baratas, medidas, de valor imediato: a entrada na sala deixa de arrastar numa demonstração, e o SCIM passa a aguentar um cliente a sério.
3. `A-03` — o push. Sem ele, tudo o resto vale menos do que devia.
4. `A-04` — **medir** onde a sala quebra. É a medição que decide se o produto compete fora do nicho do estúdio.
5. `A-09` e `A-08` — o webinar (todas as peças já existem) e o *legal hold* (o que fecha a venda no segmento que a soberania abre).
