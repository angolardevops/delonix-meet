import { describe, expect, it } from 'vitest'
import { SIMULCAST_ENCODINGS } from '../webrtc'
import {
  bestLayer,
  contentHintFor,
  decideProfile,
  DEGRADE_AFTER_MS,
  degradationFor,
  downgradeReason,
  encodingsFor,
  INITIAL_PROFILE_STATE,
  parseSendStats,
  restoreDelayMs,
  RESTORE_BASE_MS,
  RESTORE_MAX_MS,
  SHARP_TOP_BITRATE,
  UPLINK_WARMUP_MS,
  topLayerIndex,
  type ProfileState,
} from './sendProfile'

describe('downgradeReason — as condições que layerPolicy já mede', () => {
  it('rede e dispositivo bons: pode', () => {
    expect(downgradeReason({ lossPct: 1, rttMs: 80 }, 5000)).toBeNull()
  })
  it('cada condição tem o seu motivo', () => {
    expect(downgradeReason({ dataSaver: true })).toBe('saver')
    expect(downgradeReason({ preference: 'data-saver' })).toBe('saver')
    expect(downgradeReason({ batteryLow: true })).toBe('battery')
    expect(downgradeReason({ lossPct: 3.5 })).toBe('loss')
    expect(downgradeReason({ rttMs: 450 })).toBe('rtt')
    expect(downgradeReason({ cpuLimited: true })).toBe('cpu')
    expect(downgradeReason({}, 1800)).toBe('uplink')
  })
  it('limiares iguais aos de layerPolicy (perda > 3 %, RTT > 400 ms)', () => {
    expect(downgradeReason({ lossPct: 3 })).toBeNull()
    expect(downgradeReason({ rttMs: 400 })).toBeNull()
  })
  it('banda desconhecida não conta como banda má', () => {
    expect(downgradeReason({}, null)).toBeNull()
    expect(downgradeReason({}, 0)).toBeNull()
  })
  it('a escolha do utilizador vem antes da rede', () => {
    expect(downgradeReason({ dataSaver: true, lossPct: 20 })).toBe('saver')
  })
})

describe('decideProfile — histerese nos dois sentidos', () => {
  const pedir = (s: ProfileState, r: Parameters<typeof decideProfile>[2], t: number) => decideProfile(true, s, r, t)

  it('pedido com condições boas: nítido de imediato', () => {
    expect(pedir(INITIAL_PROFILE_STATE, null, 0).active).toBe('sharp')
  })
  it('pedido com condições más: fica normal e diz porquê', () => {
    const s = pedir(INITIAL_PROFILE_STATE, 'loss', 0)
    expect(s.active).toBe('normal')
    expect(s.reason).toBe('loss')
  })
  it('um pico não desliga o perfil', () => {
    let s = pedir(INITIAL_PROFILE_STATE, null, 0)
    s = pedir(s, 'rtt', 1000)
    s = pedir(s, 'rtt', 1000 + DEGRADE_AFTER_MS - 1)
    expect(s.active).toBe('sharp')
    s = pedir(s, null, 6000)
    expect(s.badSince).toBeNull()
  })
  it('condição má sustentada: volta ao normal', () => {
    let s = pedir(INITIAL_PROFILE_STATE, null, 0)
    s = pedir(s, 'loss', 1000)
    s = pedir(s, 'loss', 1000 + DEGRADE_AFTER_MS)
    expect(s.active).toBe('normal')
    expect(s.reason).toBe('loss')
    expect(s.downgrades).toBe(1)
  })
  it('banda estimada baixa não impede a entrada (encoder app-limited nunca a deixa subir)', () => {
    const s = pedir(INITIAL_PROFILE_STATE, 'uplink', 0)
    expect(s.active).toBe('sharp')
    expect(s.activeSince).toBe(0)
  })
  it('banda baixa só conta depois do aquecimento, e depois conta como as outras', () => {
    let s = pedir(INITIAL_PROFILE_STATE, null, 0)
    s = pedir(s, 'uplink', 5000)
    s = pedir(s, 'uplink', UPLINK_WARMUP_MS - 1)
    expect(s.active).toBe('sharp')
    expect(s.badSince).toBeNull()
    s = pedir(s, 'uplink', UPLINK_WARMUP_MS)
    s = pedir(s, 'uplink', UPLINK_WARMUP_MS + DEGRADE_AFTER_MS)
    expect(s.active).toBe('normal')
    expect(s.reason).toBe('uplink')
    expect(s.activeSince).toBeNull()
  })
  it('só regressa depois do período bom, e a espera cresce a cada desistência', () => {
    let s: ProfileState = { ...INITIAL_PROFILE_STATE, reason: 'cpu', downgrades: 1 }
    s = pedir(s, null, 0)
    expect(s.active).toBe('normal')
    s = pedir(s, null, RESTORE_BASE_MS - 1)
    expect(s.active).toBe('normal')
    s = pedir(s, null, RESTORE_BASE_MS)
    expect(s.active).toBe('sharp')

    expect(restoreDelayMs(0)).toBe(0)
    expect(restoreDelayMs(2)).toBe(RESTORE_BASE_MS * 2)
    expect(restoreDelayMs(20)).toBe(RESTORE_MAX_MS)
  })
  it('uma condição má durante a espera recomeça a contagem', () => {
    let s: ProfileState = { ...INITIAL_PROFILE_STATE, reason: 'cpu', downgrades: 1 }
    s = pedir(s, null, 0)
    s = pedir(s, 'cpu', 10_000)
    s = pedir(s, null, 11_000)
    s = pedir(s, null, 11_000 + RESTORE_BASE_MS - 1)
    expect(s.active).toBe('normal')
  })
  it('desligar pelo utilizador é imediato e não esquece as desistências', () => {
    const s = decideProfile(false, { ...INITIAL_PROFILE_STATE, active: 'sharp', downgrades: 2 }, null, 0)
    expect(s.active).toBe('normal')
    expect(s.downgrades).toBe(2)
  })
})

describe('encodingsFor — dentro do simulcast que já existe', () => {
  it('o instantâneo usado nos testes é o simulcast real', () => {
    expect(SIMULCAST_ENCODINGS.map((e) => e.rid)).toEqual(['q', 'h', 'f'])
  })
  it('só a camada alta muda; q e h ficam como estavam', () => {
    const out = encodingsFor('sharp', SIMULCAST_ENCODINGS)
    expect(out).toHaveLength(3)
    expect(out.map((e) => e.rid)).toEqual(['q', 'h', 'f'])
    expect(out[0]).toEqual(SIMULCAST_ENCODINGS[0])
    expect(out[1]).toEqual(SIMULCAST_ENCODINGS[1])
    expect(out[2].maxBitrate).toBe(SHARP_TOP_BITRATE)
    expect(out[2].maxBitrate!).toBeGreaterThan(SIMULCAST_ENCODINGS[2].maxBitrate!)
    expect(out[2].maxFramerate).toBe(30)
  })
  it('não muta o instantâneo (é o que repõe o normal)', () => {
    const snap = SIMULCAST_ENCODINGS.map((e) => ({ ...e }))
    encodingsFor('sharp', snap)
    expect(snap).toEqual(SIMULCAST_ENCODINGS)
  })
  it('normal devolve o instantâneo tal-qual', () => {
    expect(encodingsFor('normal', SIMULCAST_ENCODINGS)).toEqual(SIMULCAST_ENCODINGS)
  })
  it('sem simulcast (mesh): a única encoding é a alta', () => {
    expect(topLayerIndex([{ maxBitrate: 6_000_000 }])).toBe(0)
    expect(encodingsFor('sharp', [{ maxBitrate: 6_000_000 }])[0].maxBitrate).toBe(SHARP_TOP_BITRATE)
  })
  it('um tecto já maior não é reduzido', () => {
    expect(encodingsFor('sharp', [{ maxBitrate: 12_000_000 }])[0].maxBitrate).toBe(12_000_000)
  })
  it('degradação e contentHint', () => {
    expect(degradationFor('sharp', { encodings: [] })).toBe('maintain-resolution')
    expect(degradationFor('normal', { encodings: [], degradationPreference: 'balanced' })).toBe('balanced')
    expect(degradationFor('normal', { encodings: [] })).toBeUndefined()
    expect(contentHintFor('sharp')).toBe('detail')
    expect(contentHintFor('normal')).toBe('motion')
  })
})

describe('parseSendStats — o que está MESMO a sair', () => {
  const base = { timestamp: 1 }
  it('lê resolução, fps e limitação por camada, e a banda disponível', () => {
    const s = parseSendStats([
      { ...base, id: 'o3', type: 'outbound-rtp', kind: 'video', rid: 'f', frameWidth: 1280, frameHeight: 720, framesPerSecond: 29.6, qualityLimitationReason: 'bandwidth', targetBitrate: 2_100_000 },
      { ...base, id: 'o1', type: 'outbound-rtp', kind: 'video', rid: 'q', frameWidth: 320, frameHeight: 180, framesPerSecond: 30, qualityLimitationReason: 'none' },
      { ...base, id: 'o2', type: 'outbound-rtp', kind: 'video', rid: 'h', frameWidth: 640, frameHeight: 360, framesPerSecond: 30 },
      { ...base, id: 'cp', type: 'candidate-pair', nominated: true, availableOutgoingBitrate: 3_400_000 },
      { ...base, id: 'a', type: 'outbound-rtp', kind: 'audio' },
    ])
    expect(s.layers.map((l) => l.rid)).toEqual(['q', 'h', 'f'])
    expect(s.layers[2]).toEqual({ rid: 'f', width: 1280, height: 720, fps: 30, limitedBy: 'bandwidth', targetKbps: 2100 })
    expect(s.availableUpKbps).toBe(3400)
    expect(bestLayer(s)?.rid).toBe('f')
  })
  it('camada parada não conta, mesmo com o frameWidth do último frame', () => {
    const s = parseSendStats([
      { ...base, id: 'o', type: 'outbound-rtp', kind: 'video', rid: 'f', framesPerSecond: 0 },
      { ...base, id: 'p', type: 'outbound-rtp', kind: 'video', rid: 'f', frameWidth: 3840, frameHeight: 2160 },
    ])
    expect(s.layers).toEqual([])
    expect(bestLayer(s)).toBeNull()
  })
})
