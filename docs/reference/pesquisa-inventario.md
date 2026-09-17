# Inventário — pesquisa, filtros, agrupamentos e paginação por ecrã

Frente de UI `frontend/pesquisa-odoo` (worktree `.worktrees/delonix-meet/pesquisa-odoo`,
a partir de `integra/validacao-l2` @ `abcf69c`). Medido no código a 2026-09-17, antes
de qualquer alteração. O contrato do servidor (`contrato-pesquisa.md`) ainda não existia
quando isto foi escrito: a coluna «Endpoint hoje» é o que `web/src/api.ts` chama.

## Como ler

- **Paginação hoje**: `nenhuma` = o endpoint devolve a colecção inteira e a UI mostra
  tudo; `limite` = o servidor corta (o resto não chega à UI); `cursor` = `page_token`.
- **Pesquisa hoje**: o que a caixa de pesquisa faz agora.
- **Modo do painel**:
  - `servidor` — filtros, agrupamentos e página vão ao servidor (precisa do contrato);
  - `local-completo` — o servidor devolve a colecção INTEIRA (sem corte), e o painel
    filtra, agrupa e pagina o que já chegou. Não é a pesquisa profunda e não se
    apresenta como tal: não procura em transcrições nem em nada que o endpoint não
    devolva;
  - `local-truncado` — proibido: filtrar uma amostra cortada dava contagens falsas.
    Onde o servidor corta, o painel espera pelo contrato.

## Tabela

| Ecrã | Lista | Endpoint hoje | Paginação hoje | Pesquisa / filtros hoje | Campos pesquisáveis desejados | Filtros desejados | Agrupar por desejado | Modo até ao contrato |
|---|---|---|---|---|---|---|---|---|
| Gravações | biblioteca (tabela/grelha) | `GET /api/recordings` (`recordingsLibrary`) | nenhuma | caixa `q` local (nome, sala, autor) + chips (minhas, partilhadas, videoaulas, emissões, reuniões, 4K, a processar, falhadas) | Nome, Sala, Autor; **Transcrição** (só servidor) | Minhas, Partilhadas comigo, categoria, estado (pronta/publicada/a processar/falhada), 4K, Hoje, Últimos 7 dias, Este mês | Autor, Categoria, Estado, Sala, Data (dia/semana/mês/ano) | local-completo; transcrição = servidor |
| Agenda | vista Lista (e filtro das vistas Dia/Semana/Mês/Ano) | `GET /api/meetings` (`listMeetings`) | nenhuma | nenhuma | Título, Organizador, Sala, Código | Organizo eu, Convidado, Por responder, Aceites, Recusadas, Vídeo, Voz, Recorrentes, Hoje, Esta semana, Este mês, Passadas, Futuras | Organizador, Tipo, Resposta, Sala, Data (dia/semana/mês/ano) | local-completo |
| Contactos/Chamadas | contactos | `GET /api/orgs/{id}/employees` (`listEmployees`) | nenhuma | caixa local (nome/email) + filial | Nome, Email, Cargo, Filial | Online, Administradores, Membros, Recebe SMS | Filial, Papel | local-completo |
| Contactos/Chamadas | grupos | `listGroups` | nenhuma | caixa local | Nome | — | — | local-completo |
| Contactos/Chamadas | histórico | presença (`missed`) + `GET voice/cdr` (`listVoiceCdr`, admin) | CDR: nenhuma (7 dias) | nenhuma | Número, Pessoa | Perdidas, Telefone (PSTN), Hoje, Ontem | Tipo, Dia | local-completo (janela de 7 dias escrita no ecrã) |
| Quadros | biblioteca | `GET /api/whiteboards` (`listWhiteboards`) | nenhuma | caixa local (título, sala) + chips públicos/privados | Título, Sala | Públicos, Privados, Hoje, Últimos 7 dias, Este mês | Sala, Visibilidade, Data | local-completo |
| Administração | organizações | `GET /api/orgs` (`myOrgs`) | nenhuma | nenhuma | Nome, Domínio | Administro, Membro | Papel | local-completo |
| Administração | membros | `listEmployees` | nenhuma | caixa local + chips papel | Nome, Email, Cargo, Filial | Administradores, Membros, Activos 7 dias | Filial, Papel | local-completo |
| Administração | auditoria | `GET /api/orgs/{id}/audit?limit=N` (`listAudit`) | **limite** 50/100/500 | nenhuma | Actor, Acção, Alvo | Hoje, 7 dias, 30 dias, por acção | Acção, Actor, Data | **servidor** (hoje o corte impede local) |
| Integrações | webhooks | `listWebhooks` | nenhuma | nenhuma | URL, Eventos | Tipo (slack/teams/mattermost/generic), Activos | Tipo | local-completo |
| Integrações | chaves de API | `listApiKeys` | nenhuma | nenhuma | Nome, Prefixo | Nunca usadas | — | local-completo |
| Integrações | **entregas de webhooks** | — **não existe** | — | — | — | — | — | sem backend: não aparece |
| Análise | organizadores / quarentena (listas ordenadas) | `orgStats` / `quarantineAnalytics` (agregados do servidor) | já agregadas (top N) | nenhuma | Nome | período (só servidor) | — | pesquisa local sobre as linhas; período = servidor |
| Estúdio | destinos de emissão | locais (`studio/destinosLocais.ts`); `listStreamDestinations` existe em `api.ts` mas nenhum ecrã o usa | nenhuma | nenhuma | Nome, Plataforma | Activos | Plataforma | local-completo |
| Estúdio | exportações (histórico) | IndexedDB deste browser (`edit/bd.ts`) | nenhuma | nenhuma | Nome do ficheiro | Predefinição, Hoje/7 dias | Predefinição, Data | local-completo (dados do dispositivo, dito no ecrã) |
| Estúdio | biblioteca ao abrir gravação (`edit/Bin.tsx::Biblioteca`) | `recordingsLibrary` | nenhuma | ver ficheiro | Nome, Sala | Minhas, Prontas | — | local-completo |
| Início | próximas reuniões | `listMeetings` | nenhuma (corta no cliente: próximas) | nenhuma | — | — | — | sem painel; «ver todas» leva à Agenda/Lista |
| Início | gravações recentes | `recordingsLibrary` | nenhuma (corta no cliente) | nenhuma | — | — | — | sem painel; «ver todas» leva às Gravações |

## Pesquisa global (Ctrl/Cmd+K)

Hoje (`components/CommandPalette.tsx`): comandos de navegação + reuniões e gravações
**filtradas no cliente** sobre `listMeetings`/`recordingsLibrary` (sem transcrições,
sem mensagens, sem quadros) + pessoas por `GET /api/users/search`. O atalho só existe
dentro do `Shell` (a sala não o tem) e **não** respeita campos de texto (Ctrl+K dentro
de um input abre a paleta). A pesquisa profunda (transcrição com marca temporal,
mensagens, quadros) precisa de `GET /api/search` do contrato.

## Altura (antes) — `pesquisa-odoo/antes.json`

Medido com `pesquisa-odoo/medir-altura.mjs` (conta ana.mbala, API 8190). Destaques:
Agenda Semana a 1920×1080 deixa **303 px vazios** por baixo da grelha, que rola por
dentro 628 px; a 1440×900, 123 px. Administração rola a página inteira (764–3179 px)
e a auditoria rola por dentro (2442 px). Integrações e Análise rolam a página.

## Depois (frente `frontend/pesquisa-odoo`)

Contrato seguido: `docs/reference/pesquisa.md` do ramo `delonix-meet-backend/pesquisa-profunda`
(ADR-0007). Cada lista tenta primeiro o recurso do servidor (`/api/search/schemas/{resource}`);
com 404 cai na colecção INTEIRA de sempre, com «Filtrado neste browser» no ecrã e sem favoritos.

| Ecrã / lista | Fonte com o servidor do contrato | Sem o recurso no servidor | O que falta no backend |
|---|---|---|---|
| Gravações | `recordings` (transcrição, categoria, duração, favoritos) | biblioteca inteira: título, sala, autor, estado, tamanho, data | — |
| Agenda (Lista + filtro das grelhas) | `meetings` | `GET /api/meetings` inteiro | — |
| Contactos · pessoas | `members` | `/employees` inteiro | presença (online) não é campo filtrável |
| Contactos · grupos | local | local | recurso `groups` (não está no contrato) |
| Contactos · histórico | local (perdidas + CDR 7 dias) | local | `call_records` (fase 2); chamadas efectuadas/recebidas entre contactos não se guardam |
| Quadros | `whiteboards` (inclui «Os meus», dono) | lista inteira, sem dono | — |
| Administração · organizações | local | local | recurso `orgs` (não está no contrato) |
| Administração · membros | `members` | `/employees` inteiro | — |
| Administração · auditoria | `audit_events` | lista de sempre com «últimos N», SEM painel (a amostra é cortada) | — |
| Integrações · webhooks | `webhooks` quando existir (fase 2) | lista inteira | recurso `webhooks` (fase 2); registo de entregas não existe |
| Integrações · chaves de API | local | local | recurso `api_keys` (não está no contrato) |
| Análise · organizadores, quarentena | local (linhas já agregadas) | local | filtro de período das agregações é do servidor |
| Estúdio · biblioteca ao abrir gravação | `recordings` (estado só no diálogo) | lista inteira | — |
| Estúdio · histórico de exportações | local (IndexedDB do dispositivo) | local | fila de exportação no servidor não existe |
| Estúdio · destinos de emissão | sem painel | sem painel | ≤ 5 cartões ligados por índice ao editor; `stream_destinations` (fase 2) não é usado pela UI |
| Início · próximas, recentes | sem painel (pré-visualizações curtas) | — | — |

Altura (antes → depois, `bodyScroll` / vazio por baixo, conta ana.mbala na API 8190):
Agenda Semana 1920×1080 0/303 → 0/20; Agenda Lista 1440×900 324/0 → 0/20 (rola a lista);
Administração 1440×900 1077/0 → 0/16 (rola cada coluna); Integrações e Análise continuam a
rolar a página (painéis de configuração/indicadores que não cabem em 1080 px sem regiões
de scroll aninhadas).
