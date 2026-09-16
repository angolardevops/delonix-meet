// Perfil de ENVIO «Aula/apresentação nítida».
//
// O problema que resolve: quem dá uma aula à frente de um quadro, de um caderno
// ou de uma peça que mostra à câmara precisa que o detalhe chegue, e a política
// por omissão do browser para uma câmara é a contrária — quando a banda ou o
// processador apertam, o Chrome mantém a fluidez e baixa a RESOLUÇÃO
// (`maintain-framerate`, porque a track vai com `contentHint: 'motion'`). Para
// uma cara a falar é a escolha certa; para um quadro escrito é a errada.
//
// O que o perfil muda, e só isto:
//   · a câmara é pedida a 1080p/30 (`ideal`, nunca `min`: uma câmara que não dá
//     1080p continua a funcionar com o que dá);
//   · `contentHint: 'detail'` e `degradationPreference: 'maintain-resolution'` —
//     sob pressão perde-se fps, não pixels;
//   · a camada ALTA do simulcast que já existe (`f`) ganha tecto de bitrate
//     maior e fica limitada a 30 fps. (Prioridade de rede por camada NÃO: o
//     Chrome recusa `priority`/`networkPriority` diferentes entre encodings —
//     «unimplemented parameter», medido a 2026-09-16.) As camadas `q`
//     e `h` não se tocam: quem está numa rede fraca continua a receber a
//     camada leve, e é o SFU que decide qual (ver `layerPolicy.ts`).
//
// O que NÃO faz: `maxBitrate` é um TECTO, não um piso. Se a estimativa de banda
// do WebRTC não chegar lá, não há perfil que a faça chegar. Por isso a medição
// (`parseSendStats`) é parte do perfil e não um extra — sem ela o botão seria
// uma promessa.
//
// Volta sozinho ao normal quando as condições que `layerPolicy` já mede se
// degradam (perda, RTT, CPU, poupança de dados, bateria) ou quando a banda de
// envio disponível fica abaixo do que 1080p precisa — e só regressa depois de um
// período bom, com espera crescente, para não oscilar.

import type { LocalConditions } from '../layerPolicy'
import type { StatEntry } from '../callQuality'

export type SendProfile = 'normal' | 'sharp'

export type DowngradeReason = 'saver' | 'battery' | 'loss' | 'rtt' | 'cpu' | 'uplink'

/** Banda de envio abaixo da qual 1080p nítido não é sustentável (kbps). */
export const SHARP_MIN_UPLINK_KBPS = 2500
/** Tecto da camada alta no perfil nítido (bps). O normal é 6 Mbps. */
export const SHARP_TOP_BITRATE = 8_000_000
/** Uma condição má tem de durar isto antes de se desistir do perfil. */
export const DEGRADE_AFTER_MS = 4000
/** Espera base antes de voltar a tentar; duplica a cada desistência. */
export const RESTORE_BASE_MS = 15_000
export const RESTORE_MAX_MS = 120_000
/**
 * Depois de passar a nítido, a banda disponível só conta passado este tempo.
 *
 * Medido contra o servidor a sério (2026-09-16): a `availableOutgoingBitrate`
 * é a estimativa do controlo de congestão, e um encoder que envia pouco
 * («app-limited») nunca a deixa subir — a câmara falsa a 4K mandava só a camada
 * `q` e a estimativa ficava abaixo de 2,5 Mbps para sempre. Usá-la como
 * condição de ENTRADA fazia o perfil nunca ligar; usá-la logo à saída fazia-o
 * desistir durante a subida de banda que ele próprio provoca.
 */
export const UPLINK_WARMUP_MS = 15_000

/**
 * Porque é que o perfil nítido não se aguenta AGORA. `null` = pode.
 *
 * A ordem é a de quem manda: o que o utilizador escolheu para o dispositivo
 * (poupar dados, bateria) antes do que a rede mede.
 */
export function downgradeReason(cond: LocalConditions, availableUpKbps: number | null = null): DowngradeReason | null {
  if (cond.dataSaver || cond.preference === 'data-saver') return 'saver'
  if (cond.batteryLow) return 'battery'
  if ((cond.lossPct ?? 0) > 3) return 'loss'
  if ((cond.rttMs ?? 0) > 400) return 'rtt'
  if (cond.cpuLimited) return 'cpu'
  if (availableUpKbps != null && availableUpKbps > 0 && availableUpKbps < SHARP_MIN_UPLINK_KBPS) return 'uplink'
  return null
}

export interface ProfileState {
  active: SendProfile
  /** Porque é que está em `normal` apesar de ter sido pedido. */
  reason: DowngradeReason | null
  badSince: number | null
  goodSince: number | null
  /** Quantas vezes desistiu nesta sessão — alonga a espera para voltar. */
  downgrades: number
  /** Quando passou a nítido (para o aquecimento da estimativa de banda). */
  activeSince: number | null
}

export const INITIAL_PROFILE_STATE: ProfileState = {
  active: 'normal',
  reason: null,
  badSince: null,
  goodSince: null,
  downgrades: 0,
  activeSince: null,
}

export function restoreDelayMs(downgrades: number): number {
  if (downgrades <= 0) return 0
  return Math.min(RESTORE_MAX_MS, RESTORE_BASE_MS * 2 ** (downgrades - 1))
}

/**
 * Decide o perfil activo. Pura: recebe o instante, não o lê.
 *
 * Histerese nos dois sentidos. Desistir exige a condição má durante
 * `DEGRADE_AFTER_MS` (um pico de RTT não desliga nada); voltar exige a
 * condição boa durante uma espera que DUPLICA a cada desistência — um portátil
 * que aquece a 1080p deixaria de outro modo o perfil a ligar e desligar a cada
 * vinte segundos, que é pior do que qualquer dos dois perfis.
 */
export function decideProfile(wanted: boolean, prev: ProfileState, reason: DowngradeReason | null, now: number): ProfileState {
  if (!wanted) return { ...INITIAL_PROFILE_STATE, downgrades: prev.downgrades }

  if (prev.active === 'sharp') {
    const warming = prev.activeSince != null && now - prev.activeSince < UPLINK_WARMUP_MS
    const r = reason === 'uplink' && warming ? null : reason
    if (!r) return { ...prev, badSince: null, reason: null }
    const badSince = prev.badSince ?? now
    if (now - badSince < DEGRADE_AFTER_MS) return { ...prev, badSince }
    return { active: 'normal', reason: r, badSince: null, goodSince: null, downgrades: prev.downgrades + 1, activeSince: null }
  }

  // Em `normal` com o perfil pedido: ou acabou de ser pedido, ou desistiu.
  // A banda estimada não impede a ENTRADA (ver `UPLINK_WARMUP_MS`): depois de
  // uma desistência por banda, a nova tentativa espera `restoreDelayMs`.
  const blocking = reason === 'uplink' ? null : reason
  if (blocking) return { ...prev, reason: blocking, goodSince: null, badSince: null }
  const goodSince = prev.goodSince ?? now
  if (now - goodSince >= restoreDelayMs(prev.downgrades)) {
    return { active: 'sharp', reason: null, badSince: null, goodSince: null, downgrades: prev.downgrades, activeSince: now }
  }
  return { ...prev, goodSince }
}

/** O que se guarda do sender ANTES de o perfil lhe tocar, para o repor tal-qual. */
export interface SenderSnapshot {
  encodings: RTCRtpEncodingParameters[]
  degradationPreference?: RTCDegradationPreference
}

/** Índice da camada alta: a `f` do simulcast, ou a única que houver. */
export function topLayerIndex(encodings: readonly RTCRtpEncodingParameters[]): number {
  const f = encodings.findIndex((e) => e.rid === 'f')
  if (f >= 0) return f
  // Sem rid: sem simulcast (mesh, ou browser sem `sendEncodings`). A camada
  // inteira é a que não é reduzida.
  let best = 0
  for (let i = 1; i < encodings.length; i++) {
    if ((encodings[i].scaleResolutionDownBy ?? 1) < (encodings[best].scaleResolutionDownBy ?? 1)) best = i
  }
  return best
}

/** Os campos de cada encoding que o perfil muda — e só estes. */
export const TUNED_FIELDS = ['maxBitrate', 'maxFramerate'] as const

/**
 * Encodings do perfil, derivados do instantâneo. O número e a ordem das
 * encodings e os `rid` nunca mudam — o `setParameters` recusa-o.
 */
export function encodingsFor(profile: SendProfile, snapshot: readonly RTCRtpEncodingParameters[]): RTCRtpEncodingParameters[] {
  const out = snapshot.map((e) => ({ ...e }))
  if (profile === 'normal') return out
  const top = out[topLayerIndex(out)]
  if (!top) return out
  top.maxBitrate = Math.max(top.maxBitrate ?? 0, SHARP_TOP_BITRATE)
  // 30 fps chegam para uma aula; os bits que sobram vão para detalhe.
  top.maxFramerate = 30
  return out
}

export function degradationFor(profile: SendProfile, snapshot: SenderSnapshot): RTCDegradationPreference | undefined {
  return profile === 'sharp' ? 'maintain-resolution' : snapshot.degradationPreference
}

export function contentHintFor(profile: SendProfile): 'detail' | 'motion' {
  return profile === 'sharp' ? 'detail' : 'motion'
}

/** Câmara no perfil nítido: 1080p/30 quando a câmara o dá. `ideal` — nunca `min`. */
export const SHARP_CAMERA_CONSTRAINTS: MediaTrackConstraints = {
  width: { ideal: 1920 },
  height: { ideal: 1080 },
  frameRate: { ideal: 30, max: 30 },
}

/** Uma camada tal como o browser diz que a está MESMO a enviar. */
export interface SentLayer {
  rid: string
  width: number
  height: number
  fps: number
  /** `qualityLimitationReason` — `none`, `bandwidth`, `cpu` ou `other`. */
  limitedBy: string
  targetKbps: number | null
}

export interface SendStats {
  layers: SentLayer[]
  /** Banda de envio que o controlo de congestão estima, kbps. */
  availableUpKbps: number | null
}

const n = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0)

/**
 * Extrai do `getStats()` de UM sender de vídeo o que ele está a enviar.
 * Pura, sobre um array simples — o mesmo contrato de `extractQuality`.
 *
 * Camadas sem `frameWidth` ou sem fps ficam de fora: são as que o encoder
 * desligou (o Chrome desliga `h`/`f` quando a banda não chega) e mostrá-las como
 * resolução enviada seria mentir sobre o que sai.
 */
export function parseSendStats(entries: StatEntry[]): SendStats {
  const layers: SentLayer[] = []
  let available: number | null = null
  for (const s of entries) {
    if (s.type === 'outbound-rtp' && (s.kind === 'video' || s.mediaType === 'video')) {
      const width = n(s.frameWidth)
      const height = n(s.frameHeight)
      // Uma camada PARADA mantém o `frameWidth` do último frame: sem fps, não
      // está a sair nada — medido com a câmara falsa a 4K, em que a `f` ficava
      // em 3840×2160 a 0 fps e parecia a melhor camada.
      if (!width || !height || n(s.framesPerSecond) <= 0) continue
      layers.push({
        rid: typeof s.rid === 'string' ? s.rid : '',
        width,
        height,
        fps: Math.round(n(s.framesPerSecond)),
        limitedBy: typeof s.qualityLimitationReason === 'string' ? s.qualityLimitationReason : 'none',
        targetKbps: typeof s.targetBitrate === 'number' ? Math.round(s.targetBitrate / 1000) : null,
      })
    } else if (s.type === 'candidate-pair' && (s.nominated || s.selected || s.state === 'succeeded')) {
      if (typeof s.availableOutgoingBitrate === 'number') available = Math.round(s.availableOutgoingBitrate / 1000)
    }
  }
  const order = (rid: string) => (rid === 'q' ? 0 : rid === 'h' ? 1 : rid === 'f' ? 2 : 3)
  layers.sort((a, b) => order(a.rid) - order(b.rid) || a.width - b.width)
  return { layers, availableUpKbps: available }
}

/** A camada de maior resolução que está mesmo a sair. `null` sem amostras. */
export function bestLayer(stats: SendStats | null): SentLayer | null {
  if (!stats || stats.layers.length === 0) return null
  return stats.layers.reduce((a, b) => (b.width * b.height > a.width * a.height ? b : a))
}
