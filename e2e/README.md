# Testes ponta-a-ponta — arnês de media, rede degradada, isolamento

> **Os scripts vivem em `web/e2e/`, não aqui.** É onde o Node resolve
> `node_modules` — correr `node e2e/isolamento.mjs` a partir da raiz falha com
> `ERR_MODULE_NOT_FOUND`, porque as dependências (`ws`, `@playwright/test`)
> estão em `web/`. Este directório guarda o `Dockerfile.netem` e esta nota.

## Isolamento entre inquilinos (`web/e2e/isolamento.mjs`)

Duas organizações independentes contra um servidor a sério, e para cada recurso
verifica-se que a org A não alcança o que é da org B. Corre no CI, com um
Postgres de serviço.

```bash
docker compose up -d postgres
cargo build --release --manifest-path server/Cargo.toml
DELONIX_ALLOW_INSECURE=1 ./server/target/release/delonix-server &
node web/e2e/isolamento.mjs
```

**Uma expectativa que estava errada, e vale a pena saber porquê.** A primeira
versão exigia que a org A levasse `403` ao ler uma sala da org B. Leva `200` — e
está certo: o código da sala é uma **capability** à maneira do Meet, quem o
conhece pode ver os metadados e PEDIR para entrar. A invariante que interessa não
é «A é recusado», é **A nunca obtém acesso DIRECTO à media de outra
organização**. Verificado no fio: o dono recebe `joined`, a org A recebe
`waiting`. É essa a asserção que o teste faz hoje.

---

# Arnês de media e rede degradada

O que aqui está corre contra **infraestrutura real**: Postgres, Redis, o SFU em
Rust e dois Chromium a sério. Não há duplos de teste no caminho de media.

## Porquê ao nível do arnês e não da interface

`web/e2e/harness.html` carrega a **pilha real do cliente** — o mesmo `SfuCall` e
o mesmo `Signaling` que a aplicação usa — sem passar pela interface. Um teste
que carrega em botões mede a interface e parte quando um botão muda de sítio; o
que aqui é preciso medir é **media**: se os pacotes atravessam, com que
qualidade, e se recuperam quando a rede parte.

## Porquê `tc netem` e não o estrangulamento do DevTools

Nem o `--use-fake-network` do Chrome nem o estrangulamento do DevTools afectam
**UDP**, e é UDP que transporta a media. Estrangular só o HTTP dá um número
bonito e sem relação nenhuma com a chamada. O `netem` molda o caminho real.

## Como correr

```bash
# 1. Infra
docker compose up -d postgres redis

# 2. Servidor. Para os testes de rede degradada tem de ser em CONTENTOR: o `tc`
#    precisa de CAP_NET_ADMIN, e não se dá isso ao host.
cargo build --release --manifest-path server/Cargo.toml
cp server/target/release/delonix-server e2e/ && docker build -t delonix-netem:test -f e2e/Dockerfile.netem e2e/
docker run -d --name dlx-srv --network dlx-netem --cap-add=NET_ADMIN -p 8180:8180 \
  -e DELONIX_ALLOW_INSECURE=1 -e COOKIE_INSECURE=1 \
  -e DATABASE_URL='postgres://delonix:delonix_dev@<pg>:5432/delonix_meet' \
  delonix-netem:test

# 3. Frontend (contexto seguro em localhost, sem TLS)
cd web && NO_HTTPS=1 npx vite --port 5173

# 4. Matriz
node web/e2e/netem-matrix.mjs
node web/e2e/netem-matrix.mjs --only perda-10%
SETTLE_MS=20000 node web/e2e/netem-matrix.mjs
```

## LIMITAÇÃO CONHECIDA deste arnês — ler antes de confiar em qualquer número

**Com o servidor em contentor, a ligação ICE cai de forma reprodutível cerca de
10 segundos depois de estabelecer.** O registo do servidor mostra
`sfu pc state … state=connecting` a esse tempo, e a media não volta.

Medido a 2026-08-25, e a comparação que fecha a questão: com o **mesmo binário
a correr no host**, a mesma chamada fica estável durante 40 segundos
(20 amostras, um único soluço num dos peers, recuperado). **Portanto o problema
é do transporte pela bridge do Docker, não do produto.** Não foi encontrada a
causa exacta — as suspeitas são o par peer-reflexive sobre a bridge e o
tratamento de conntrack para o UDP efémero do SFU.

Consequência prática: **a matriz completa de cenários NÃO é publicável a partir
deste arnês.** A janela útil é curta demais para uma medição estável de doze
cenários.

O que a matriz JÁ provou, e que vale a pena guardar: quando mede dentro da
janela estável, **mede com exactidão**.

| netem aplicado | perda medida | score |
|---|---|---|
| `loss 10%` | 10,2 % | 55 |
| `loss 20%` | 21 % | 55 |

A cadeia de medição está certa; falta-lhe um transporte estável.

## O que é preciso para a matriz completa

Uma de duas coisas, nenhuma delas ao alcance desta máquina:

1. **Browser e SFU no MESMO namespace de rede** (o `netem` aplicado ao `lo`
   partilhado), o que dispensa a bridge — exige uma imagem com Chromium; ou
2. **Duas máquinas de verdade**, com o `netem` na interface física de uma.

## Armadilhas que já custaram caro aqui

- **Não sondar o `getStats()` mais depressa do que ~1×/s.** O Chrome actualiza
  as estatísticas cerca de uma vez por segundo; duas leituras dentro do mesmo
  intervalo têm o mesmo carimbo temporal, o delta dá zero, e o teste conclui
  «sem media» numa chamada saudável. Custou uma matriz inteira de falsos
  negativos, o cenário de referência incluído.
- **O par de candidatos escolhido vem do `transport.selectedCandidatePairId`**,
  não de `state === 'succeeded'`. Ver o comentário em `web/src/callQuality.ts`:
  procurar `succeeded` devolvia `null` em 16 de 16 amostras contra Chromium a
  sério, e com o par nulo a métrica de uso de TURN respondia «nunca» para
  sempre.
- **A imagem do arnês é `ubuntu:24.04`, não `debian:12`.** O binário é
  compilado no host contra a glibc 2.39; o debian:12 traz a 2.36 e o contentor
  morre com `GLIBC_2.39 not found`.
