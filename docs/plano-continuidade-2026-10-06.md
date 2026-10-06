# Plano de continuidade — fechar o ciclo que ficou aberto em sessões paralelas

**Data:** 2026-10-06 · **Ramo medido:** `origin/develop` (`eadc38fe`) · **Método:** `gh pr list`,
`git rev-list --count HEAD --not --remotes=origin` e `git status --porcelain` em cada um dos 32
worktrees desta máquina, mais o inventário das sessões abertas no Claude Code.

**Para que serve.** Treze sessões trabalharam no Meet em paralelo a 5 de Outubro. Este documento
diz **onde cada uma ficou**, o que estava **só no disco** (e por isso não existia para mais
ninguém), e em que **ordem** se fecha o que resta. O backlog de produto não se repete aqui: a
[auditoria de lacunas de 2026-10-04](plano-lacunas-2026-10-04.md) e o
[plano de produção de 2026-10-03](plano-producao-2026-10-03.md) continuam a ser a fonte — este
plano aponta para os itens deles e dá-lhes ordem.

**O que correu hoje, e o que não.** Correu: `gh` contra o GitHub, a leitura dos 32 worktrees,
`cargo fmt --check` e `cargo check --all-targets` nas árvores que foram empurradas, e os portões
`check-docs-drift.sh` e `check-repo-hygiene.sh` na árvore do #235. **Não** correu: a bateria de
testes, nenhum e2e, nenhum browser, nenhuma chamada, nenhum cluster — a máquina reiniciou às
22:59 de 2026-10-05 e o laboratório estava em baixo. Onde este plano diz que algo funciona, a
prova é a do CI na PR respectiva, não uma medição feita aqui.

---

## 1. Onde cada frente ficou

### 1.1 As doze PRs abertas

Todas contra a `develop`. «CI» é o estado às 11:30 de 2026-10-06.

| PR | O que fecha | CI | Notas |
|---|---|---|---|
| [#232](https://github.com/angolardevops/delonix-meet/pull/232) | Uma gravação a compor lê-se «a processar», não «falhada» | verde | Base da pilha das gravações |
| [#242](https://github.com/angolardevops/delonix-meet/pull/242) | O painel da sala sabe o estado de cada gravação | verde | **Contém o #232** |
| [#238](https://github.com/angolardevops/delonix-meet/pull/238) | O pacote de áudio atrasado escreve-se no sítio | verde | `recorder.rs` |
| [#245](https://github.com/angolardevops/delonix-meet/pull/245) | Cada pista entra na gravação no instante do seu primeiro pacote | novo | `recorder.rs`; **recuperado do disco hoje** |
| [#235](https://github.com/angolardevops/delonix-meet/pull/235) | `OUTBOUND_ALLOW_HOSTS` em todo o lado, e o portão de deriva a vê-lo | verde | Estava **em conflito**; resolvido hoje |
| [#243](https://github.com/angolardevops/delonix-meet/pull/243) | O `prune-builds.sh` diz porque parou (seguimento da R303) | verde | |
| [#236](https://github.com/angolardevops/delonix-meet/pull/236) | A mesma pessoa no browser e ao telefone escolhe onde continuar | verde | `Room.tsx` e `locales/*/room.ts`, como o #242 |
| [#244](https://github.com/angolardevops/delonix-meet/pull/244) | Tecto de sockets por conta e quota de participantes por organização (ADR-0017) | novo | **Recuperado do disco hoje**; migração `0097` |
| [#247](https://github.com/angolardevops/delonix-meet/pull/247) | As vagas de composição e o lote de repetições repartem-se por inquilino | novo | **Recuperado do disco hoje**; `fair_slots.rs` era inédito |
| [#246](https://github.com/angolardevops/delonix-meet/pull/246) | Os restos da R295: o silêncio escrito ao gravar | **rascunho** | **Não funde como está** — ver o Sprint 2 |
| [#248](https://github.com/angolardevops/delonix-meet/pull/248) | Este documento | novo | |
| [#249](https://github.com/angolardevops/delonix-meet/pull/249) | `source-map-js` 1.2.2 | novo | **Funde primeiro** — ver abaixo |

**O portão de dependências estava vermelho em todas elas, e não por causa de nenhuma.** O
[GHSA-68fv-2mgg-jv7q](https://github.com/advisories/GHSA-68fv-2mgg-jv7q) (ALTO) saiu a 2026-10-06 e
apanha o `source-map-js` 1.0.0–1.2.1, transitiva do `postcss` pelo Vite. O `check-dep-audit.sh` dá
vermelho a qualquer aviso novo, por isso o job «Segurança das dependências» caiu de uma vez em PRs
que estavam verdes uma hora antes. Corrigido no #249 com um `overrides` para a 1.2.2 — não foi para
a lista de aceites porque há correcção, e aceitar o que se pode corrigir é o que torna essa lista
inútil quando aparecer um aviso que importe.

### 1.2 O que estava só no disco

Quatro frentes tinham trabalho que **não existia em `origin`**. Um reinício da máquina levava-as —
e a máquina já reiniciou uma vez neste ciclo.

| Onde estava | O que era | Para onde foi |
|---|---|---|
| `tectos-ws-e-quota-org` (5 commits) | tectos do `/ws` por conta e por organização | #244 |
| `gravacao-sincronismo` (3 commits + 1 linha por commitar) | o instante de cada pista na composição | #245, cherry-pick limpo sobre a `develop` |
| `justica-webhooks-gravacoes` (7 ficheiros **nunca commitados**) | `fair_slots.rs` e a repartição do lote de webhooks | #247 |
| `gravacao-silencio-na-pista` (5 commits) | os restos da R295 | #246 (rascunho: conflita com a R299) |

E duas coisas que continuam só no disco, de propósito:

- **A medição de carga de 2026-10-05** (`.worktrees/delonix-meet/carga-tecto/.carga/`): uma rampa
  até **40 pessoas numa sala** — vídeo 1560/1560, perda 0,03 %, jitter95 6,45 ms, **2833 Mbps**,
  servidor a **11,17 cores**, RSS 2929 MB, com a máquina a 81 %. É o item **X2** do plano de
  lacunas, e está em `.carga/resultados/` sem nenhum documento que o cite. Entra no Sprint 2.
- **Um `recorder.rs` esvaziado** (3535 linhas apagadas) no worktree `gravacao-sincronismo`,
  acidente de uma sessão interrompida. Não foi commitado nem descartado: fica para o dono decidir.

### 1.3 Worktrees — o que morre e o que fica

Trinta e dois worktrees. Depois de hoje, **nenhum guarda trabalho que não esteja em `origin`** —
os quatro que guardavam estão em §1.2 — e por isso todos se destroem no fim do Sprint 1, com duas
excepções. Os dez `.claude/worktrees/*` das sessões saem com o arquivamento delas; as `integra/*`
saem à medida que a PR respectiva funde.

Ficam de pé, e não se destroem:

- **`laboratorio`** — é de onde o compose e o cluster locais correm. Os seus 11 commits são só
  merges; o conteúdo é idêntico à `develop` (`git diff --stat origin/develop` vazio).
- **`carga-tecto`** — guarda os artefactos da medição de carga até o Sprint 2 os escrever.

**Higiene:** `canais/`, `ponte-pstn/` e `v3-canais/` estão dentro de `.worktrees/delonix-meet/` mas
são checkouts do **`ngolacloud-harness`**, não deste repo, cada um com `.claude/settings.json` e
`ci/gates.toml` modificados. Não pertencem aqui e não são deste plano.

### 1.4 As sessões

Dezesseis sessões com trabalho neste repo. Nove fecharam com PR fundida (#163, #168, #185, #189,
#221, #224, #227, #233, #240, #241), três ficaram com PR aberta (#232, #235, #242), e duas — «Levar
a espera pelo keyframe à linha do tempo da pista gravada» e «Escrever silêncio na pista de áudio ao
gravar» — trabalharam **fora do worktree que lhes foi dado**, que ficou vazio: o resultado delas
estava nos worktrees de §1.2 e só hoje saiu da máquina. Todas são arquivadas depois do Sprint 1.

---

## 2. Os sprints

A ordem conta. O Sprint 1 é dívida de entrega: nada de novo até o que está provado estar na
`develop`.

### Sprint 1 — Aterrar o que já está provado

**Ordem de fusão.** As PRs tocam cinco vezes o mesmo `docs/reference/regressions.md` e duas vezes
o mesmo `recorder.rs`: fundidas noutra ordem, cada fusão põe a seguinte em conflito. E nenhuma fica
verde antes do #249.

0. **#249** — sem ele nenhuma das outras fica verde, nem a `develop`.
1. **#232** — base da pilha.
2. **#242** — contém o #232; depois dele o #232 fica vazio.
3. **#238** — primeiro dos dois que mexem no `recorder.rs`.
4. **#245** — o outro; imediatamente depois do #238, para o conflito do writer se resolver de uma só vez.
5. **#235** — conflito do catálogo já resolvido; re-conflita se esperar.
6. **#243**.
7. **#236** — depois do #242, por causa de `Room.tsx` e dos `locales/*/room.ts`.
8. **#244** — migração `0097`: confirmar que ainda é o número livre à hora da fusão.
9. **#247**.

**Depois de cada fusão:** CI verde na cabeça da `develop`, não só na PR.

**Ao fim:** destruir os worktrees de §1.3 (todos menos o `laboratorio` e o `carga-tecto`), apagar
as `integra/*` do remoto e arquivar as 16 sessões. Uma tarefa com worktree vivo não está terminada.

### Sprint 2 — Fechar a gravação

A frente mais quente do ciclo: cinco PRs deste sprint tocaram o gravador, e o que resta são as
pontas que nenhuma delas fechou.

1. **Integrar o #246** — conflito **medido**, não suposto: `sync_channel::<Box<Packet>>` da
   `develop` contra o `Box<Queued>` desta branch, e o `try_send` que a R299 usa para decidir se a
   pista abriu no keyframe. O `Queued` tem de passar a carregar o predicado do keyframe. Prova: o
   e2e do pedaço de fala solto antes do silêncio, com o laboratório de pé.
2. **Medir o #245 no laboratório** depois de fundido — os cinco cenários da régua de som e imagem
   correram antes do rebase, não depois.
3. **Os 20 ms iniciais de um toque com DTX** continuam fora do sítio (R295, por fechar). É o que
   sobra depois de 1 e 2.
4. **A S3 aberta em `access()`** apanhada na revisão do #242: uma gravação arquivada deixa ler a transcrição
   por um caminho que não confere o que devia. Medido, não corrigido.
5. **Escrever a medição de carga** de §1.2 em `docs/ops/teste-de-carga-2026-10-05.md`, com o
   commit medido e a carga do host — fecha metade do **X2** sem precisar de hardware novo.

### Sprint 3 — As decisões que só o dono toma

Onze decisões (**D1–D11** do plano de lacunas) e o **V5** (oito ADR com código fundido e estado
ainda «Proposto»). Não são trabalho de agente e **travam os Sprints 4 a 7**: sem a D2, as gravações
em objectos não se desenham; sem a D3, há dois modelos de TV a coexistir; sem a D4, uma operadora
que entregue RTP em claro não nos consegue ligar; sem a D8, a onda de TV ordena-se por suposição.

A mais barata e a mais urgente é a **D9/V5**: fixar o estado dos ADR custa uma tarde e é o que
torna o resto do repo legível.

### Sprint 4 — Instalar e operar (onda 1)

Pela ordem do caminho crítico nº 1 da auditoria: **O2** (o CI constrói, publica por digest e
assina), **O8** (`/ready` mede Postgres e Redis), **O10** (TURN por TCP/TLS na 443 com mais de uma
réplica), **O6** (backup com restauro testado), **O4** (instalar o chart em produção num cluster
limpo, com duas réplicas) e, do frontend, **F1** (o build deixa de embutir tudo em `data:`, que
choca com a CSP) e **F2** (error boundary por rota). O **O4** depende da **D1**.

### Sprint 5 — Adopção enterprise (onda 2)

**E2** e **E3** (correio, reposição de password: sem elas o convidado e o administrador ficam
pendurados), depois **E5** ou **E4** conforme a D6, e **E8**, **E9**, **E10** (gravações em
objectos, ciclo de vida dos dados, auditoria completa). O **E1** fechou a 2026-10-05 (R290).

### Sprint 6 — Operadoras (onda 3)

**T3** (perfil de tronco no repo), **T4** (a decisão D4 aplicada), **T5** (a **primeira operadora**
— nenhuma chamada passou por uma até hoje), **T9** (bordo de produção) e o **resto do T11**: o #218
e a R300 abriram o ESL e a propagação de troncos, mas dois FreeSWITCH no dispatcher, o `INVITE` ao
pod que tem a sala e o FreeSWITCH sem root continuam abertos.

### Sprint 7 — Estúdio e TV (onda 4)

Só depois da **D3** e da **D8**. Ordem: **TV1** (ligar as dez rotas do estúdio ao ecrã — hoje sem
um único consumidor), **TV2**, **TV3**, **TV4** (a emissão que não morre com o browser do
anfitrião). Até ao TV4 não se anuncia «Estúdio de TV para canais».

### Sprint 8 — Escala (onda 5)

**X3** (RTCP SR com NTP — o vizinho natural do Sprint 2), **X5** (os crates em falta do ADR-0006) e
o **X2** completo, em hardware dedicado. O **X1** fechou com o lugar em Redis (#214).

---

## 3. O que este plano não valida

- **Nada foi exercitado.** Nem bateria, nem e2e, nem browser, nem chamada, nem cluster. As três PRs
  recuperadas hoje (#244, #245, #247) têm `fmt` e `cargo check` limpos nesta máquina e mais nada:
  o veredicto é o do CI nelas.
- **A ordem de fusão do Sprint 1 é previsão**, feita dos ficheiros que cada PR toca. O primeiro
  conflito real pode mudá-la.
- **A integração do #246 não foi tentada** além do cherry-pick que produziu o conflito citado.
- **Os tamanhos e a duração de cada sprint não estão estimados**: não há capacidade de equipa
  medida, e a auditoria de 2026-10-04 já o disse dos seus próprios P/M/G.
- **O `recorder.rs` esvaziado** de §1.2 não foi analisado: não se sabe se a sessão que o esvaziou
  tinha outra intenção.
