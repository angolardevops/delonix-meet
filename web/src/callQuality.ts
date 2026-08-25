// Qualidade de chamada: extracção das métricas do `getStats()` e o **Delonix
// Call Quality Score**.
//
// Porque é que isto existe: a auditoria de 2026-08-25 mediu que se recolhiam
// TRÊS números (RTT, perda, uplink) das cerca de vinte e cinco que o §4.4 do
// mandato pede. Com três números não há diagnóstico — «a chamada estava má» não
// distingue rede do cliente, CPU do cliente, ou o nó a servir camada a mais. E,
// sobretudo, não há SLO defensável: uma meta publicada sobre uma medição que não
// existe é uma meta inventada.
//
// Tudo aqui é PURO. Recebe um objecto com a forma do `RTCStatsReport` e devolve
// números; nenhum acesso ao browser, nenhuma promessa. É o que permite testar os
// casos que só acontecem com a rede em baixo.

/** Uma entrada do `RTCStatsReport`, com os campos que nos interessam. */
export interface StatEntry {
  id: string
  type: string
  timestamp: number
  [k: string]: unknown
}

/** Estado entre amostras — os contadores do WebRTC são CUMULATIVOS. */
export interface QualityCursor {
  bytes: number
  packets: number
  lost: number
  nack: number
  pli: number
  fir: number
  framesDropped: number
  freezeMs: number
  concealed: number
  samples: number
  ts: number
}

export interface PeerQuality {
  /** Downlink deste publicador, kbps. */
  kbps: number
  /** Perda no intervalo, %. */
  lossPct: number
  /** Jitter em ms (instantâneo, do último relatório). */
  jitterMs: number
  /** Frames descartados no intervalo. */
  framesDropped: number
  /** Tempo de imagem congelada no intervalo, ms. */
  freezeMs: number
  /** Fracção de amostras de áudio ocultadas (PLC) no intervalo, 0..1. */
  concealmentRatio: number
  nack: number
  pli: number
  fir: number
}

export interface QualitySample {
  /** RTT ao SFU, ms. */
  rttMs: number | null
  /** Jitter agregado (o pior dos publicadores), ms. */
  jitterMs: number
  /** Perda agregada (a pior), %. */
  lossPct: number
  /** Uplink próprio, kbps. */
  upKbps: number
  /** Downlink total, kbps. */
  downKbps: number
  /** A media está a passar por TURN relay? */
  turnRelay: boolean
  /** Tipo do par de candidatos escolhido, ex.: "host/srflx", "relay/relay". */
  candidatePair: string | null
  /** Banda de saída estimada pelo controlo de congestão, kbps. */
  availableUpKbps: number | null
  /** O encoder está limitado por quê: "cpu" | "bandwidth" | "none" | … */
  limitedBy: string | null
  /** Congelamento total no intervalo, ms (pior publicador). */
  freezeMs: number
  /** Ocultação de áudio no intervalo, 0..1 (pior publicador). */
  concealmentRatio: number
  framesSent: number
  framesReceived: number
  framesDropped: number
  nack: number
  pli: number
  fir: number
  byPeer: Record<string, PeerQuality>
}

const EMPTY_CURSOR: QualityCursor = {
  bytes: 0, packets: 0, lost: 0, nack: 0, pli: 0, fir: 0,
  framesDropped: 0, freezeMs: 0, concealed: 0, samples: 0, ts: 0,
}

const num = (v: unknown, dflt = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : dflt)
const kbps = (deltaBytes: number, deltaMs: number): number =>
  deltaMs > 0 ? Math.max(0, Math.round((deltaBytes * 8) / deltaMs)) : 0

/**
 * Extrai uma amostra de qualidade a partir das entradas do `getStats()`.
 *
 * `cursors` é lido E actualizado: os contadores do WebRTC são cumulativos desde
 * o início da sessão, por isso tudo o que interessa é o DELTA face à amostra
 * anterior. Uma média desde o início da chamada esconde exactamente aquilo que
 * queremos ver — os dez segundos maus no meio de meia hora boa.
 */
export function extractQuality(
  entries: StatEntry[],
  cursors: Map<string, QualityCursor>,
  /** Regex que identifica o publicador no `trackIdentifier` posto pelo SFU. */
  peerIdOf: (trackIdentifier: string) => string | null = defaultPeerIdOf,
): QualitySample {
  const out: QualitySample = {
    rttMs: null, jitterMs: 0, lossPct: 0, upKbps: 0, downKbps: 0,
    turnRelay: false, candidatePair: null, availableUpKbps: null, limitedBy: null,
    freezeMs: 0, concealmentRatio: 0,
    framesSent: 0, framesReceived: 0, framesDropped: 0, nack: 0, pli: 0, fir: 0,
    byPeer: {},
  }

  const byId = new Map(entries.map((e) => [e.id, e]))

  for (const s of entries) {
    switch (s.type) {
      case 'candidate-pair': {
        if (s.state !== 'succeeded' && s.selected !== true) break
        if (typeof s.currentRoundTripTime === 'number') {
          out.rttMs = Math.round(num(s.currentRoundTripTime) * 1000)
        }
        if (typeof s.availableOutgoingBitrate === 'number') {
          out.availableUpKbps = Math.round(num(s.availableOutgoingBitrate) / 1000)
        }
        const local = byId.get(String(s.localCandidateId ?? ''))
        const remote = byId.get(String(s.remoteCandidateId ?? ''))
        const lt = local ? String(local.candidateType ?? '?') : '?'
        const rt = remote ? String(remote.candidateType ?? '?') : '?'
        out.candidatePair = `${lt}/${rt}`
        // TURN em USO — não «TURN configurado». É a diferença entre saber que
        // temos relay e saber que a media está mesmo a pagar o desvio por ele.
        out.turnRelay = lt === 'relay' || rt === 'relay'
        break
      }

      case 'outbound-rtp': {
        const prev = cursors.get(s.id) ?? EMPTY_CURSOR
        const bytes = num(s.bytesSent)
        const dt = s.timestamp - prev.ts
        if (prev.ts > 0) out.upKbps += kbps(bytes - prev.bytes, dt)
        out.framesSent += Math.max(0, num(s.framesSent) - prev.framesDropped)
        out.nack += Math.max(0, num(s.nackCount) - prev.nack)
        out.pli += Math.max(0, num(s.pliCount) - prev.pli)
        out.fir += Math.max(0, num(s.firCount) - prev.fir)
        // `qualityLimitationReason` é o único sinal de CPU que o browser dá de
        // forma portável: diz se o encoder baixou a qualidade e porquê.
        const reason = typeof s.qualityLimitationReason === 'string' ? s.qualityLimitationReason : null
        if (reason && reason !== 'none') out.limitedBy = reason
        cursors.set(s.id, {
          ...EMPTY_CURSOR, ts: s.timestamp, bytes,
          framesDropped: num(s.framesSent),
          nack: num(s.nackCount), pli: num(s.pliCount), fir: num(s.firCount),
        })
        break
      }

      case 'inbound-rtp': {
        const peer = peerIdOf(String(s.trackIdentifier ?? ''))
        const prev = cursors.get(s.id) ?? EMPTY_CURSOR
        const dt = s.timestamp - prev.ts
        const bytes = num(s.bytesReceived)
        const packets = num(s.packetsReceived)
        const lost = num(s.packetsLost)
        const freezeMs = num(s.totalFreezesDuration) * 1000
        const concealed = num(s.concealedSamples)
        const samples = num(s.totalSamplesReceived)

        const dKbps = prev.ts > 0 ? kbps(bytes - prev.bytes, dt) : 0
        const dPackets = Math.max(0, packets - prev.packets)
        const dLost = Math.max(0, lost - prev.lost)
        // Perda do INTERVALO. A perda acumulada desde o início da chamada
        // aproxima-se de uma constante e deixa de reagir — inútil para alertar.
        const lossPct = dPackets + dLost > 0 ? (100 * dLost) / (dPackets + dLost) : 0
        const dFreeze = Math.max(0, freezeMs - prev.freezeMs)
        const dConcealed = Math.max(0, concealed - prev.concealed)
        const dSamples = Math.max(0, samples - prev.samples)
        const concealmentRatio = dSamples > 0 ? dConcealed / dSamples : 0
        const jitterMs = Math.round(num(s.jitter) * 1000)
        const dDropped = Math.max(0, num(s.framesDropped) - prev.framesDropped)

        out.downKbps += dKbps
        out.framesReceived += Math.max(0, num(s.framesReceived))
        out.framesDropped += dDropped
        out.nack += Math.max(0, num(s.nackCount) - prev.nack)
        out.pli += Math.max(0, num(s.pliCount) - prev.pli)
        out.fir += Math.max(0, num(s.firCount) - prev.fir)
        // Agregados = o PIOR publicador, não a média. Uma sala com nove pessoas
        // boas e uma inaudível tem um problema; a média esconde-o.
        out.lossPct = Math.max(out.lossPct, lossPct)
        out.jitterMs = Math.max(out.jitterMs, jitterMs)
        out.freezeMs = Math.max(out.freezeMs, dFreeze)
        out.concealmentRatio = Math.max(out.concealmentRatio, concealmentRatio)

        cursors.set(s.id, {
          bytes, packets, lost, ts: s.timestamp,
          nack: num(s.nackCount), pli: num(s.pliCount), fir: num(s.firCount),
          framesDropped: num(s.framesDropped), freezeMs, concealed, samples,
        })

        if (peer) {
          const cur = out.byPeer[peer]
          out.byPeer[peer] = {
            kbps: (cur?.kbps ?? 0) + dKbps,
            lossPct: Math.max(cur?.lossPct ?? 0, Math.round(lossPct * 10) / 10),
            jitterMs: Math.max(cur?.jitterMs ?? 0, jitterMs),
            framesDropped: (cur?.framesDropped ?? 0) + dDropped,
            freezeMs: (cur?.freezeMs ?? 0) + dFreeze,
            concealmentRatio: Math.max(cur?.concealmentRatio ?? 0, concealmentRatio),
            nack: (cur?.nack ?? 0) + Math.max(0, num(s.nackCount) - prev.nack),
            pli: (cur?.pli ?? 0) + Math.max(0, num(s.pliCount) - prev.pli),
            fir: (cur?.fir ?? 0) + Math.max(0, num(s.firCount) - prev.fir),
          }
        }
        break
      }
    }
  }

  out.lossPct = Math.round(out.lossPct * 10) / 10
  return out
}

/** O SFU põe `trackIdentifier = "<publisher-uuid>-<kind>-<rid>"`. */
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/
export function defaultPeerIdOf(trackIdentifier: string): string | null {
  return UUID_RE.test(trackIdentifier) ? trackIdentifier.slice(0, 36) : null
}

// ---------------------------------------------------------------------------
//  Delonix Call Quality Score
// ---------------------------------------------------------------------------

/**
 * Pontuação 0–100 de uma amostra.
 *
 * **O que isto é:** um modelo de penalizações TRANSPARENTE, com limiares
 * ancorados em referências públicas — a ITU-T G.114 põe 150 ms de atraso num
 * sentido como o limite do «bom» e 400 ms como o do aceitável; a perda acima de
 * 5% degrada inteligibilidade de forma audível mesmo com PLC.
 *
 * **O que isto NÃO é:** não é MOS, não é o E-model da G.107, e **não está
 * calibrado contra julgamento humano**. É uma escala interna, comparável consigo
 * mesma ao longo do tempo e entre salas. O próprio mandato o exige no §9 para o
 * áudio: não se declara qualidade só com uma métrica. Chamar-lhe MOS seria
 * emprestar-lhe uma autoridade que não tem.
 *
 * **Áudio pesa mais que vídeo, de propósito.** Numa rede degradada — o caso
 * normal do nosso mercado — a chamada é útil sem imagem e inútil sem som. A
 * ocultação de áudio (PLC) é a penalização mais dura por isso mesmo: é o número
 * que mede som que o utilizador NÃO ouviu.
 */
export function callQualityScore(s: QualitySample): number {
  let score = 100

  // Perda: 0% não penaliza; a partir daí, ~6 pontos por ponto percentual até
  // saturar. 5% de perda ⇒ -30, que põe uma chamada em «fraca».
  score -= Math.min(45, s.lossPct * 6)

  // RTT (ida-e-volta ao SFU). Até 150 ms não penaliza — é o «bom» da G.114 lido
  // como round-trip conservador. Depois, 1 ponto por cada 20 ms.
  if (s.rttMs != null && s.rttMs > 150) score -= Math.min(20, (s.rttMs - 150) / 20)

  // Jitter: até 30 ms o buffer absorve. Acima, 1 ponto por cada 10 ms.
  if (s.jitterMs > 30) score -= Math.min(15, (s.jitterMs - 30) / 10)

  // Ocultação de áudio: o utilizador PERDEU som. É a penalização mais dura por
  // ponto — 10% de ocultação já custa 25 pontos.
  score -= Math.min(25, s.concealmentRatio * 250)

  // Imagem congelada no intervalo. 1 s congelado num intervalo de 2 s é metade
  // da chamada sem vídeo.
  score -= Math.min(15, s.freezeMs / 100)

  // Encoder travado por CPU: o problema é a máquina, não a rede — mas o
  // participante vê-o na mesma, e sem esta parcela ficava invisível.
  if (s.limitedBy === 'cpu') score -= 8

  return Math.max(0, Math.min(100, Math.round(score)))
}

/** Faixas para leitura humana. Os limites são os do cartão do admin. */
export type QualityBand = 'boa' | 'aceitável' | 'fraca' | 'má'
export function qualityBand(score: number): QualityBand {
  if (score >= 80) return 'boa'
  if (score >= 60) return 'aceitável'
  if (score >= 40) return 'fraca'
  return 'má'
}
