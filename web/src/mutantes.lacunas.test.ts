// As lacunas que o teste por mutação encontrou (`scripts/mutantes.mjs`).
//
// Cada `describe` aqui existe porque uma mutação SOBREVIVEU: o código foi
// alterado de propósito e a bateria inteira continuou verde. Não eram testes em
// falta por distração — eram fronteiras e guardas que ninguém tinha prometido
// por escrito, e por isso ninguém defendia.
//
// O comentário de cada teste diz QUAL foi a mutação que sobreviveu, para quem
// vier a seguir poder repetir a experiência em vez de acreditar.
import { describe, it, expect } from 'vitest'
import { extractQuality, type StatEntry } from './callQuality'
import { chooseLayers, type TileSignal, type LocalConditions } from './layerPolicy'
import { LinhaDoTempo } from './callTimings'

describe('guardas numéricas do callQuality', () => {
  // MUTAÇÃO SOBREVIVENTE: `typeof v === 'number' && Number.isFinite(v)` → `||`.
  // Com `||`, um `NaN` passa a guarda (porque `typeof NaN === 'number'`) e
  // entra nas contas. Um NaN numa métrica propaga-se a TUDO o que a use: a
  // pontuação de qualidade, a média, o gráfico. E não rebenta — mostra-se.
  it('um NaN vindo do getStats não entra na amostra', () => {
    const entries: StatEntry[] = [
      { type: 'candidate-pair', id: 'p1', timestamp: 0, state: 'succeeded', currentRoundTripTime: NaN },
      { type: 'inbound-rtp', id: 'i1', timestamp: 0, kind: 'audio', jitter: NaN, packetsLost: NaN, packetsReceived: 100 },
    ]
    const s = extractQuality(entries, new Map())
    expect(Number.isFinite(s.jitterMs)).toBe(true)
    expect(Number.isFinite(s.lossPct)).toBe(true)
    expect(s.rttMs === null || Number.isFinite(s.rttMs)).toBe(true)
  })
})

describe('escolha do par de candidatos (é o que diz se há TURN)', () => {
  // TRÊS mutações sobreviveram na mesma linha:
  //   `s.state === 'succeeded' || s.selected === true || s.nominated === true`
  // com `||`→`&&` e dois `===`→`!==`. Ou seja: NADA testava por qual par a
  // media passa — e é essa a fonte do `turnRelay`, que a consola mostra ao
  // utilizador e o `/metrics` publica.
  const par = (id: string, extra: Record<string, unknown> = {}): StatEntry => ({
    type: 'candidate-pair', id, timestamp: 0, localCandidateId: `L${id}`, remoteCandidateId: `R${id}`,
    bytesSent: 1000, bytesReceived: 1000, ...extra,
  })
  const cand = (id: string, tipo: string, tp = 'local-candidate'): StatEntry =>
    ({ type: tp, id, timestamp: 0, candidateType: tipo })

  it('o `transport` manda: o par que ele aponta é o escolhido, mesmo com outro «succeeded»', () => {
    const entries: StatEntry[] = [
      { type: 'transport', id: 't', timestamp: 0, selectedCandidatePairId: 'bom' },
      par('mau', { state: 'succeeded' }),
      par('bom'),
      cand('Lmau', 'host'),
      cand('Lbom', 'relay'),
      cand('Rbom', 'relay', 'remote-candidate'),
      cand('Rmau', 'host', 'remote-candidate'),
    ]
    const s = extractQuality(entries, new Map())
    expect(s.candidatePair).toBe('relay/relay')
    expect(s.turnRelay).toBe(true)
  })

  it('sem `transport`, um par «succeeded» serve de recuo', () => {
    const entries: StatEntry[] = [
      par('p', { state: 'succeeded' }),
      cand('Lp', 'relay'),
      cand('Rp', 'host', 'remote-candidate'),
    ]
    const s = extractQuality(entries, new Map())
    expect(s.candidatePair).toBe('relay/host')
    expect(s.turnRelay).toBe(true)
  })

  // MUTAÇÕES SOBREVIVENTES na mesma linha: `s.selected === true` → `!==` e
  // `s.nominated === true` → `!==`. Sobreviviam porque os pares dos testes
  // acima não DEFINEM esses campos: com `undefined`, tanto `=== true` como
  // `!== true` deixam a cadeia `||` verdadeira. Só um par que diga
  // explicitamente «não fui escolhido» distingue as duas versões.
  it('um par que se declara NÃO escolhido não é usado', () => {
    const entries: StatEntry[] = [
      par('rejeitado', { state: 'failed', selected: false, nominated: false }),
      cand('Lrejeitado', 'relay'),
      cand('Rrejeitado', 'relay', 'remote-candidate'),
    ]
    const s = extractQuality(entries, new Map())
    expect(s.candidatePair).toBe(null)
    expect(s.turnRelay).toBe(false)
  })

  // MUTAÇÃO SOBREVIVENTE: no filtro do `transport`,
  // `e.type === 'transport' && typeof e.selectedCandidatePairId === 'string'`
  // → `||`. Com `||`, um `transport` SEM `selectedCandidatePairId` passa a
  // contar e devolve `undefined` — e o recuo por «succeeded» deixa de correr,
  // porque o código julga que já tem resposta.
  it('um `transport` sem par escolhido não desliga o recuo', () => {
    const entries: StatEntry[] = [
      { type: 'transport', id: 't', timestamp: 0 },
      par('p', { state: 'succeeded' }),
      cand('Lp', 'relay'),
      cand('Rp', 'host', 'remote-candidate'),
    ]
    const s = extractQuality(entries, new Map())
    expect(s.candidatePair).toBe('relay/host')
    expect(s.turnRelay).toBe(true)
  })

  it('um par que NÃO está escolhido não conta — senão o TURN aparecia sem estar em uso', () => {
    const entries: StatEntry[] = [
      { type: 'transport', id: 't', timestamp: 0, selectedCandidatePairId: 'host' },
      par('host'),
      par('relay', { state: 'succeeded' }),
      cand('Lhost', 'host'),
      cand('Lrelay', 'relay'),
      cand('Rhost', 'host', 'remote-candidate'),
      cand('Rrelay', 'relay', 'remote-candidate'),
    ]
    const s = extractQuality(entries, new Map())
    expect(s.candidatePair).toBe('host/host')
    expect(s.turnRelay).toBe(false)
  })
})

describe('fronteiras do orçamento de banda', () => {
  const tile = (id: string, extra: Partial<TileSignal> = {}): TileSignal => ({
    peerId: id, widthPx: 640, ...extra,
  })
  const cond = (extra: Partial<LocalConditions> = {}): LocalConditions => ({
    downlinkKbps: null, ...extra,
  })

  // MUTAÇÃO SOBREVIVENTE: `budget != null && budget > 0` → `||`.
  // Com `||`, um orçamento de ZERO entra no bloco e degrada tudo ao mínimo. É
  // a diferença entre «não sei a banda» e «não há banda», e um `0` vindo de uma
  // API que ainda não mediu é a primeira coisa.
  it('um orçamento de zero é tratado como DESCONHECIDO, não como «sem banda»', () => {
    const tiles = [tile('a', { onStage: true }), tile('b')]
    const semSaber = chooseLayers(tiles, cond({ downlinkKbps: null }))
    const zero = chooseLayers(tiles, cond({ downlinkKbps: 0 }))
    expect(zero).toEqual(semSaber)
  })

  // MUTAÇÃO SOBREVIVENTE: `custo() <= budget` → `<`.
  // Na igualdade exacta, `<` continua a degradar mais um degrau sem precisar.
  it('quando o custo bate CERTO com o orçamento, não se degrada mais nada', () => {
    const tiles = [tile('a', { onStage: true }), tile('b'), tile('c')]
    const largo = chooseLayers(tiles, cond({ downlinkKbps: 100000 }))
    // Um orçamento generoso não pode dar um resultado pior do que não ter
    // orçamento nenhum — e na fronteira era exactamente isso que acontecia.
    const semOrcamento = chooseLayers(tiles, cond({ downlinkKbps: null }))
    expect(largo).toEqual(semOrcamento)
  })
})

describe('duração zero é um dado, não um buraco', () => {
  // MUTAÇÃO SOBREVIVENTE: `b >= a` → `b > a`.
  // Dois marcos no MESMO milissegundo dão duração 0, que é um valor legítimo.
  // Com `>` passava a `null`, e um `null` num painel lê-se como «não medido».
  it('dois marcos no mesmo instante dão 0 ms e não null', () => {
    // O relógio é injectado: os dois marcos caem no MESMO instante, que é o
    // caso que a fronteira `>=` cobre e o `>` deitava fora.
    const t = new LinhaDoTempo(() => 1000)
    t.marcar('intencao')
    t.marcar('ws')
    expect(t.entre('intencao', 'ws')).toBe(0)
  })
})
