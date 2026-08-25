import { describe, it, expect, beforeEach, vi } from 'vitest'
import { SfuCall } from './webrtc'
import type { RecoveryConfig } from './callRecovery'

/**
 * A metade de ligamento da recuperação de chamada.
 *
 * O `callRecovery.test.ts` prova a DECISÃO (pura). Este prova que a `SfuCall`
 * a executa: que uma PC em `failed` produz mesmo uma oferta com
 * `iceRestart: true`, que um soluço curto NÃO produz nenhuma, e que desligar
 * não dispara recuperação. Sem este, a máquina de estados podia estar perfeita
 * e não haver um único `restartIce` a sair pelo fio.
 *
 * Antes desta alteração não havia nada disto: a recuperação de rede era
 * `location.reload()`.
 */

type Desc = { type: string; sdp?: string }

/** Config minúscula para os testes correrem em milissegundos, não em segundos. */
const FAST: RecoveryConfig = { graceMs: 30, baseMs: 5, maxMs: 10, maxAttempts: 3 }

class FakePC {
  signalingState = 'stable'
  connectionState: RTCPeerConnectionState = 'new'
  localDescription: Desc | null = null
  remoteDescription: Desc | null = null
  onicecandidate: unknown = null
  ontrack: unknown = null
  onconnectionstatechange: (() => void) | null = null
  closed = false
  /** Opções de cada createOffer — é aqui que se vê o `iceRestart`. */
  offerOpts: (RTCOfferOptions | undefined)[] = []

  async createOffer(opts?: RTCOfferOptions) {
    this.offerOpts.push(opts)
    return { type: 'offer', sdp: 'v=0\r\no=- offer' }
  }
  async createAnswer() {
    return { type: 'answer', sdp: 'v=0\r\no=- answer' }
  }
  async setLocalDescription(d: Desc) {
    if (d.type === 'rollback') {
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
  close() {
    this.closed = true
    this.connectionState = 'closed'
    this.onconnectionstatechange?.()
  }

  /** Encena uma transição de estado da ligação, como faz o browser. */
  transition(next: RTCPeerConnectionState) {
    this.connectionState = next
    this.onconnectionstatechange?.()
  }
}

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

const settle = async (ms = 0) => {
  for (let i = 0; i < 20; i++) await Promise.resolve()
  await new Promise((r) => setTimeout(r, ms))
  for (let i = 0; i < 20; i++) await Promise.resolve()
}

describe('SfuCall — recuperação de media por ICE restart', () => {
  let pc: FakePC
  beforeEach(() => {
    pc = new FakePC()
    vi.stubGlobal('RTCPeerConnection', function () {
      return pc
    } as unknown as typeof RTCPeerConnection)
    vi.stubGlobal('MediaStream', class {})
  })

  /** Chamada já estabelecida e estável (negociação inicial concluída). */
  async function established() {
    const { signal, sent, emit } = fakeSignal()
    const states: string[] = []
    const stream = { getTracks: () => [], getAudioTracks: () => [], getVideoTracks: () => [] }
    const call = new SfuCall(
      signal as never,
      stream as unknown as MediaStream,
      {},
      { onStream: () => {}, onPeerLeft: () => {}, onState: (s) => states.push(s) },
      undefined,
      FAST,
    )
    await settle()
    emit('sfu-answer', { sdp: 'v=0\r\no=- server-answer' })
    await settle()
    pc.transition('connected')
    sent.length = 0
    pc.offerOpts.length = 0
    return { call, sent, emit, states }
  }

  it('`failed` produz uma oferta com iceRestart — a recuperação que não existia', async () => {
    const { sent, states } = await established()

    pc.transition('failed')
    await settle(40)

    expect(sent.map((m) => m.type)).toEqual(['sfu-offer'])
    expect(pc.offerOpts[0]).toEqual({ iceRestart: true })
    expect(states).toContain('reconnecting')
    expect(states).toContain('recovering')
  })

  it('um soluço curto NÃO renegoceia: `disconnected` seguido de `connected` dentro da graça', async () => {
    const { sent, states } = await established()

    pc.transition('disconnected')
    await settle(5) // bem dentro dos 30ms de graça
    expect(states).toContain('degraded')
    expect(sent).toHaveLength(0)

    pc.transition('connected') // o ICE recuperou sozinho, como acontece amiúde
    await settle(50) // deixa a graça expirar: não pode disparar nada

    expect(sent).toHaveLength(0)
    expect(states[states.length - 1]).toBe('connected')
  })

  it('`disconnected` que persiste para além da graça acaba em ICE restart', async () => {
    const { sent } = await established()

    pc.transition('disconnected')
    await settle(80) // graça (30) + backoff

    expect(sent.map((m) => m.type)).toEqual(['sfu-offer'])
    expect(pc.offerOpts[0]).toEqual({ iceRestart: true })
  })

  it('desligar é terminal: um `failed` depois do hangup não tenta recuperar nada', async () => {
    const { call, sent } = await established()

    call.hangup()
    sent.length = 0
    pc.transition('failed')
    await settle(80)

    expect(sent).toHaveLength(0)
    expect(pc.closed).toBe(true)
  })

  it('esgotado o orçamento, desiste em vez de renegociar para sempre', async () => {
    const { sent, states } = await established()

    // Cada `failed` consome uma tentativa; o `connected` intermédio é que as
    // repõe, e aqui nunca há nenhum.
    for (let i = 0; i < FAST.maxAttempts + 2; i++) {
      pc.transition('failed')
      await settle(40)
    }

    expect(states).toContain('failed')
    expect(sent.length).toBeLessThanOrEqual(FAST.maxAttempts)
  })

  it('uma recuperação bem sucedida repõe o orçamento para a quebra seguinte', async () => {
    const { sent, emit } = await established()

    pc.transition('failed')
    await settle(40)
    expect(sent).toHaveLength(1)

    // O servidor responde à oferta de restart (é o que o `apply_client_offer`
    // faz) e a media volta. Sem modelar esta resposta a PC ficaria em
    // `have-local-offer` — e o teste seguinte mediria o adiamento, não o
    // orçamento.
    emit('sfu-answer', { sdp: 'v=0\r\no=- restart-answer' })
    await settle()
    pc.transition('connected')
    await settle()

    // Uma quebra nova, muito depois: tem de voltar a ter tentativas.
    pc.transition('failed')
    await settle(40)
    expect(sent).toHaveLength(2)
  })

  it('não oferta enquanto há uma negociação a meio — volta a tentar em vez de falhar por estado', async () => {
    const { sent } = await established()

    // Negociação em voo: ofertar agora daria erro de estado (a PC só aceita uma
    // oferta local quando está `stable`). É o caso real de a rede cair no meio
    // de uma renegociação do servidor.
    pc.signalingState = 'have-remote-offer'

    pc.transition('failed')
    await settle(40)
    expect(sent).toHaveLength(0)

    // A negociação fecha; o restart adiado (recheck a 500ms) sai então.
    pc.signalingState = 'stable'
    await settle(700)

    expect(sent.map((m) => m.type)).toEqual(['sfu-offer'])
    expect(pc.offerOpts.some((o) => o?.iceRestart)).toBe(true)
  })
})
