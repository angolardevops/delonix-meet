# Fila de paridade — delonix-meet

Uma lacuna, um ficheiro, um contrato de sessão. Medido a **2026-10-18** contra `develop` @ `f21e6fc47`.
A tabela completa com veredictos e evidência: [`docs/paridade-medida-2026-10-18.md`](../../docs/paridade-medida-2026-10-18.md).

**Como se usa.** `/paridade <id>` imprime o prompt tal e qual, com as recusas. A sessão que o corre abre o seu próprio contrato com `/escopo`. Um id sem ficheiro **não é recusa** — é o que o `/paridade novo <id>` existe para escrever.

**O que não se faz aqui:** implementar. O prompt escreve-se nesta fila; a correcção acontece noutra sessão, com contrato próprio.

## Grupo A — bloqueia uma venda ou é afirmação falsa

| id | Lacuna | Estado do prompt |
|---|---|---|
| `A-01` | Um telefone entra numa sala cifrada de ponta a ponta | **em curso** numa sessão desta máquina (ramo `integra/dial-in-e2ee`) — não escrever prompt nem mexer |
| `A-02` | [Gravação ISO ligada por omissão e inexistente](A-02-gravacao-iso-ligada-e-inexistente.md) | escrito |
| `A-03` | [Ninguém é avisado sem ter a aba aberta](A-03-push-com-a-app-fechada.md) | escrito |
| `A-04` | [Quarenta pessoas por sala](A-04-quarenta-pessoas-por-sala.md) | escrito — **mede primeiro, decide depois** |
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
| `B-06` | [A entrada na sala é O(n) em série](B-06-entrada-na-sala-em-serie.md) | escrito — **a mais barata da lista** |
| `B-07` | API pública v1 é 3 % da BFF, e não há SDK | por escrever |
| `B-08` | A sala de espera do convidado é obrigatória e sem interruptor | por escrever |
| `B-09` | Acessibilidade sem prova automática (`grep axe` = 0) | por escrever |
| `B-10` | [O SCIM trunca em silêncio acima de 200](B-10-scim-trunca-em-silencio.md) | escrito — **melhor relação esforço/valor** |
| `B-11` | App de desktop | **defensável não ter** — ver o documento; não entra na fila sem decisão de produto |

## Grupo C — telefonia

| id | Lacuna | Estado do prompt |
|---|---|---|
| `C-01` | PBX: filas, voicemail, atendedor, supervisão, transferência | por escrever — é o maior, e precisa de decisão de produto antes do prompt |
| `C-02` | Softphone no nosso próprio produto | por escrever |
| `C-03` | Bordo de operadora (identidade, tectos de gasto, registo de entrada) | por escrever |
| `C-04` | Salas de hardware / SIP de vídeo | por escrever — **o bloqueio não é o codec**, é não haver gateway de vídeo; ver o documento |

## Porque é que onze estão por escrever

Não por esquecimento. Um prompt que ninguém corre neste mês apodrece como qualquer outro documento — as citações `ficheiro:linha` deslizam e a próxima sessão herda uma fotografia errada. Escreveram-se os **oito** que bloqueiam uma venda ou são afirmação falsa latente; os restantes escrevem-se com `/paridade novo <id>` no momento em que alguém os for fazer, com as linhas relidas nesse dia.

## Ordem sugerida, se a pergunta for «por onde começo»

1. `A-01` (em curso) e `A-02` — as duas afirmações falsas. Um produto que mente sobre si próprio não se vende duas vezes.
2. `B-06` e `B-10` — baratas, medidas, de valor imediato: a entrada na sala deixa de arrastar numa demonstração, e o SCIM passa a aguentar um cliente a sério.
3. `A-03` — o push. Sem ele, tudo o resto vale menos do que devia.
4. `A-04` — **medir** onde a sala quebra. É a medição que decide se o produto compete fora do nicho do estúdio.
5. `A-09` e `A-08` — o webinar (todas as peças já existem) e o *legal hold* (o que fecha a venda no segmento que a soberania abre).
