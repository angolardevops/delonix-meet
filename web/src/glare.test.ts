import { describe, it, expect, beforeEach, vi } from 'vitest'
import { SfuCall } from './webrtc'

/**
 * R13, metade do cliente.
 *
 * O teste ponta-a-ponta em `server/src/sfu_e2e.rs` prova que o SERVIDOR já não
 * perde a oferta do cliente durante o glare (adia-a). Mas isso, sozinho, não
 * chega: quando o cliente faz `rollback` para aceitar a oferta do servidor,
 * **descarta a sua própria oferta** — e com ela a negociação das tracks que ela
 * publicava (ecrã, câmara). A resposta que o servidor acabar por mandar a essa
 * oferta já não serve: o cliente está `stable` e o handler de `sfu-answer`
 * descarta-a. Sem uma RE-OFERTA, a partilha de ecrã desaparecia à mesma.
 *
 * Este teste não podia viver do lado do servidor: o webrtc-rs não suporta
 * rollback a partir de `have-local-offer`, logo um cliente de teste em Rust não
 * consegue encenar este caminho.
 */

type Desc = { type: string; sdp?: string }

class FakePC {
  signalingState = 'stable'
  localDescription: Desc | null = null
  remoteDescription: Desc | null = null
  onicecandidate: unknown = null
  ontrack: unknown = null
  rollbacks = 0

  async createOffer() {
    return { type: 'offer', sdp: 'v=0\r\no=- offer' }
  }
  async createAnswer() {
    return { type: 'answer', sdp: 'v=0\r\no=- answer' }
  }
  async setLocalDescription(d: Desc) {
    if (d.type === 'rollback') {
      this.rollbacks++
      this.signalingState = 'stable'
      this.localDescription = null
      return
    }
    this.localDescription = d
    this.signalingState = d.type === 'offer' ? 'have-local-offer' : 'stable'
  }
  async setRemoteDescription(d: Desc) {
    this.remoteDescription = d
    this.signalingState = d.type === 'offer' ? 'have-remote-offer' : 'stable'
  }
  async addIceCandidate() {}
  addTrack() {
    return { replaceTrack: async () => {} }
  }
  addTransceiver() {
    return { sender: { replaceTrack: async () => {} }, direction: 'sendrecv' }
  }
  getSenders(): unknown[] {
    return []
  }
  getTransceivers(): unknown[] {
    return []
  }
  removeTrack() {}
  close() {}
}

/** Sinalização falsa: guarda o que foi enviado e deixa disparar eventos. */
function fakeSignal() {
  const handlers = new Map<string, (m: unknown) => void>()
  const sent: { type: string; sdp?: string }[] = []
  return {
    signal: {
      on: (ev: string, h: (m: unknown) => void) => handlers.set(ev, h),
      send: (m: { type: string; sdp?: string }) => sent.push(m),
      close: () => {},
    },
    sent,
    emit: (ev: string, m: unknown) => handlers.get(ev)?.(m),
  }
}

/** Deixa a fila interna de SDP da SfuCall drenar. */
const settle = async () => {
  for (let i = 0; i < 20; i++) await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  for (let i = 0; i < 20; i++) await Promise.resolve()
}

describe('SfuCall — glare (R13, lado do cliente)', () => {
  let pc: FakePC
  beforeEach(() => {
    pc = new FakePC()
    vi.stubGlobal(
      'RTCPeerConnection',
      function () {
        return pc
      } as unknown as typeof RTCPeerConnection,
    )
    vi.stubGlobal('MediaStream', class {})
  })

  async function connected() {
    const { signal, sent, emit } = fakeSignal()
    const stream = { getTracks: () => [], getAudioTracks: () => [], getVideoTracks: () => [] }
    new SfuCall(
      signal as never,
      stream as unknown as MediaStream,
      {},
      { onStream: () => {}, onPeerLeft: () => {} },
    )
    await settle()
    return { sent, emit }
  }

  it('a oferta inicial sai na construção (R1) e deixa a PC em have-local-offer', async () => {
    const { sent } = await connected()
    expect(sent.map((m) => m.type)).toEqual(['sfu-offer'])
    expect(pc.signalingState).toBe('have-local-offer')
  })

  it('oferta do servidor durante a nossa: rollback, resposta E RE-OFERTA', async () => {
    const { sent, emit } = await connected()
    sent.length = 0

    // O servidor renegoceia enquanto temos uma oferta por responder.
    emit('sfu-offer', { sdp: 'v=0\r\no=- server-offer' })
    await settle()

    expect(pc.rollbacks).toBe(1)
    // Sem a re-oferta, as tracks que a nossa oferta publicava ficavam por
    // negociar para sempre — a partilha de ecrã que "não aparece".
    expect(sent.map((m) => m.type)).toEqual(['sfu-answer', 'sfu-offer'])
    expect(pc.signalingState).toBe('have-local-offer')
  })

  it('sem glare (PC estável) responde e NÃO re-oferta', async () => {
    const { sent, emit } = await connected()
    // Fecha a negociação inicial: passamos a estável.
    emit('sfu-answer', { sdp: 'v=0\r\no=- server-answer' })
    await settle()
    expect(pc.signalingState).toBe('stable')
    sent.length = 0

    emit('sfu-offer', { sdp: 'v=0\r\no=- server-offer' })
    await settle()

    expect(pc.rollbacks).toBe(0)
    // Uma re-oferta aqui seria ruído: renegociação a mais em cada subscrição.
    expect(sent.map((m) => m.type)).toEqual(['sfu-answer'])
  })
})
