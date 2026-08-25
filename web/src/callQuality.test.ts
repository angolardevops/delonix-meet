import { describe, it, expect } from 'vitest'
import {
  extractQuality,
  callQualityScore,
  qualityBand,
  defaultPeerIdOf,
  type StatEntry,
  type QualityCursor,
  type QualitySample,
} from './callQuality'

const PEER = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'

/** Amostra sem defeito nenhum — a base para variar UMA coisa de cada vez. */
const PERFECT: QualitySample = {
  rttMs: 40, jitterMs: 5, lossPct: 0, upKbps: 900, downKbps: 1400,
  turnRelay: false, candidatePair: 'srflx/srflx', availableUpKbps: 3000, limitedBy: 'none',
  freezeMs: 0, concealmentRatio: 0,
  framesSent: 60, framesReceived: 60, framesDropped: 0, nack: 0, pli: 0, fir: 0,
  byPeer: {},
}

function inbound(over: Partial<StatEntry> = {}): StatEntry {
  return {
    id: 'in-1', type: 'inbound-rtp', timestamp: 1000,
    trackIdentifier: `${PEER}-video-f`,
    bytesReceived: 0, packetsReceived: 0, packetsLost: 0,
    jitter: 0, framesReceived: 0, framesDropped: 0,
    totalFreezesDuration: 0, concealedSamples: 0, totalSamplesReceived: 0,
    nackCount: 0, pliCount: 0, firCount: 0,
    ...over,
  }
}

describe('extractQuality — os contadores são cumulativos, o que interessa é o delta', () => {
  it('a PRIMEIRA amostra não inventa bitrate (sem anterior, não há intervalo)', () => {
    const cur = new Map<string, QualityCursor>()
    const s = extractQuality([inbound({ bytesReceived: 1_000_000, packetsReceived: 800 })], cur)
    // Sem isto, a primeira amostra dividia o total da sessão por um intervalo
    // inventado e reportava um pico absurdo logo à entrada.
    expect(s.downKbps).toBe(0)
    expect(cur.size).toBe(1)
  })

  it('a segunda amostra dá o bitrate DO INTERVALO', () => {
    const cur = new Map<string, QualityCursor>()
    extractQuality([inbound({ timestamp: 1000, bytesReceived: 0 })], cur)
    // 250 000 bytes em 2000 ms = 1000 kbps.
    const s = extractQuality([inbound({ timestamp: 3000, bytesReceived: 250_000 })], cur)
    expect(s.downKbps).toBe(1000)
  })

  it('a perda é a DO INTERVALO, não a acumulada desde o início da chamada', () => {
    const cur = new Map<string, QualityCursor>()
    // Arranque mau: 100 perdidos em 1000.
    extractQuality([inbound({ timestamp: 1000, packetsReceived: 900, packetsLost: 100 })], cur)
    // Intervalo seguinte PERFEITO: +1000 recebidos, +0 perdidos.
    const s = extractQuality(
      [inbound({ timestamp: 3000, packetsReceived: 1900, packetsLost: 100 })],
      cur,
    )
    // A acumulada continuaria a dizer ~5%. A do intervalo diz a verdade: 0.
    expect(s.lossPct).toBe(0)
  })

  it('a ocultação de áudio é uma fracção do intervalo', () => {
    const cur = new Map<string, QualityCursor>()
    extractQuality([inbound({ timestamp: 1000, concealedSamples: 0, totalSamplesReceived: 0 })], cur)
    const s = extractQuality(
      [inbound({ timestamp: 3000, concealedSamples: 480, totalSamplesReceived: 96_000 })],
      cur,
    )
    expect(s.concealmentRatio).toBeCloseTo(0.005, 5)
  })

  it('congelamento em segundos vira milissegundos, e só o do intervalo', () => {
    const cur = new Map<string, QualityCursor>()
    extractQuality([inbound({ timestamp: 1000, totalFreezesDuration: 1 })], cur)
    const s = extractQuality([inbound({ timestamp: 3000, totalFreezesDuration: 1.75 })], cur)
    expect(s.freezeMs).toBeCloseTo(750, 0)
  })
})

describe('extractQuality — agregação', () => {
  it('o agregado é o PIOR publicador, não a média', () => {
    const cur = new Map<string, QualityCursor>()
    const bom = { id: 'a', trackIdentifier: `${PEER}-video-f` }
    const mau = { id: 'b', trackIdentifier: `11111111-2222-3333-4444-555555555555-video-f` }
    extractQuality(
      [inbound({ ...bom, timestamp: 1000 }), inbound({ ...mau, timestamp: 1000 })],
      cur,
    )
    const s = extractQuality(
      [
        inbound({ ...bom, timestamp: 3000, packetsReceived: 1000, packetsLost: 0, jitter: 0.004 }),
        inbound({ ...mau, timestamp: 3000, packetsReceived: 800, packetsLost: 200, jitter: 0.09 }),
      ],
      cur,
    )
    // Média daria ~10% e mascarava o participante inaudível. O pior é 20%.
    expect(s.lossPct).toBe(20)
    expect(s.jitterMs).toBe(90)
  })

  it('separa a qualidade POR publicador', () => {
    const cur = new Map<string, QualityCursor>()
    const e = inbound({ timestamp: 1000 })
    extractQuality([e], cur)
    const s = extractQuality(
      [inbound({ timestamp: 3000, packetsReceived: 900, packetsLost: 100, jitter: 0.02 })],
      cur,
    )
    expect(s.byPeer[PEER]).toBeDefined()
    expect(s.byPeer[PEER].lossPct).toBe(10)
    expect(s.byPeer[PEER].jitterMs).toBe(20)
  })

  it('ignora media sem publicador identificável (m-lines sem track do SFU)', () => {
    const cur = new Map<string, QualityCursor>()
    const s = extractQuality([inbound({ trackIdentifier: 'nao-e-uuid' })], cur)
    expect(Object.keys(s.byPeer)).toHaveLength(0)
  })
})

describe('extractQuality — caminho de rede', () => {
  const pair = (localType: string, remoteType: string): StatEntry[] => [
    {
      id: 'cp', type: 'candidate-pair', timestamp: 1000, state: 'succeeded',
      currentRoundTripTime: 0.084, availableOutgoingBitrate: 2_400_000,
      localCandidateId: 'L', remoteCandidateId: 'R',
    },
    { id: 'L', type: 'local-candidate', timestamp: 1000, candidateType: localType },
    { id: 'R', type: 'remote-candidate', timestamp: 1000, candidateType: remoteType },
  ]

  it('detecta TURN em USO (não «TURN configurado»)', () => {
    const s = extractQuality(pair('relay', 'srflx'), new Map())
    expect(s.turnRelay).toBe(true)
    expect(s.candidatePair).toBe('relay/srflx')
  })

  it('media directa não conta como relay', () => {
    const s = extractQuality(pair('srflx', 'srflx'), new Map())
    expect(s.turnRelay).toBe(false)
  })

  it('lê RTT em ms e a banda estimada em kbps', () => {
    const s = extractQuality(pair('host', 'host'), new Map())
    expect(s.rttMs).toBe(84)
    expect(s.availableUpKbps).toBe(2400)
  })
})

describe('extractQuality — limitação do encoder', () => {
  it('regista quando o encoder está travado, e por quê', () => {
    const s = extractQuality(
      [{ id: 'o', type: 'outbound-rtp', timestamp: 1000, bytesSent: 0, qualityLimitationReason: 'cpu' }],
      new Map(),
    )
    expect(s.limitedBy).toBe('cpu')
  })

  it('«none» não é uma limitação', () => {
    const s = extractQuality(
      [{ id: 'o', type: 'outbound-rtp', timestamp: 1000, bytesSent: 0, qualityLimitationReason: 'none' }],
      new Map(),
    )
    expect(s.limitedBy).toBeNull()
  })
})

describe('callQualityScore', () => {
  it('uma chamada sem defeito dá 100', () => {
    expect(callQualityScore(PERFECT)).toBe(100)
  })

  it('a perda pesa: 5% põe a chamada em «fraca»', () => {
    const s = callQualityScore({ ...PERFECT, lossPct: 5 })
    expect(s).toBe(70)
    expect(qualityBand(s)).toBe('aceitável')
    expect(callQualityScore({ ...PERFECT, lossPct: 10 })).toBeLessThan(60)
  })

  it('o áudio ocultado é a penalização mais dura — é som que o utilizador NÃO ouviu', () => {
    const comPerda = callQualityScore({ ...PERFECT, lossPct: 2 })
    const comOcultacao = callQualityScore({ ...PERFECT, concealmentRatio: 0.1 })
    expect(comOcultacao).toBeLessThan(comPerda)
  })

  it('RTT abaixo de 150 ms não penaliza (limiar da G.114)', () => {
    expect(callQualityScore({ ...PERFECT, rttMs: 149 })).toBe(100)
    expect(callQualityScore({ ...PERFECT, rttMs: 350 })).toBeLessThan(100)
  })

  it('jitter dentro do que o buffer absorve não penaliza', () => {
    expect(callQualityScore({ ...PERFECT, jitterMs: 29 })).toBe(100)
    expect(callQualityScore({ ...PERFECT, jitterMs: 130 })).toBeLessThan(95)
  })

  it('CPU travada aparece na pontuação — o participante vê-o na mesma', () => {
    expect(callQualityScore({ ...PERFECT, limitedBy: 'cpu' })).toBe(92)
    // Limitação por banda já está reflectida na perda/bitrate; não se penaliza duas vezes.
    expect(callQualityScore({ ...PERFECT, limitedBy: 'bandwidth' })).toBe(100)
  })

  it('nunca sai de 0..100, por pior que seja', () => {
    const horrivel: QualitySample = {
      ...PERFECT, lossPct: 100, rttMs: 5000, jitterMs: 2000,
      concealmentRatio: 1, freezeMs: 10_000, limitedBy: 'cpu',
    }
    const s = callQualityScore(horrivel)
    expect(s).toBeGreaterThanOrEqual(0)
    expect(s).toBeLessThanOrEqual(100)
    expect(s).toBe(0)
  })

  it('é monótona: piorar um eixo nunca melhora a pontuação', () => {
    let anterior = 101
    for (const loss of [0, 1, 2, 5, 10, 20, 50]) {
      const s = callQualityScore({ ...PERFECT, lossPct: loss })
      expect(s).toBeLessThanOrEqual(anterior)
      anterior = s
    }
  })
})

describe('qualityBand', () => {
  it('as fronteiras são as do cartão do admin', () => {
    expect(qualityBand(100)).toBe('boa')
    expect(qualityBand(80)).toBe('boa')
    expect(qualityBand(79)).toBe('aceitável')
    expect(qualityBand(60)).toBe('aceitável')
    expect(qualityBand(59)).toBe('fraca')
    expect(qualityBand(40)).toBe('fraca')
    expect(qualityBand(39)).toBe('má')
    expect(qualityBand(0)).toBe('má')
  })
})

describe('defaultPeerIdOf', () => {
  it('extrai o UUID do publicador que o SFU põe no trackIdentifier', () => {
    expect(defaultPeerIdOf(`${PEER}-video-f`)).toBe(PEER)
    expect(defaultPeerIdOf(`${PEER}-audio`)).toBe(PEER)
  })
  it('devolve null para o que não é do SFU', () => {
    expect(defaultPeerIdOf('')).toBeNull()
    expect(defaultPeerIdOf('local-camera')).toBeNull()
  })
})
