// Arnês de media para os testes ponta-a-ponta.
//
// Carrega a PILHA REAL do cliente — `SfuCall` e `Signaling`, os mesmos módulos
// que a aplicação usa — contra o SFU Rust a correr de verdade. Não é um duplo
// de teste: é o `webrtc.ts` do produto.
//
// Porque não conduzir a interface: um teste que carrega em botões mede a
// interface, e parte quando um botão muda de sítio. O que aqui é preciso medir
// é MEDIA — se os pacotes atravessam, com que qualidade, e se recuperam quando
// a rede parte. Esse nível é este.
import { SfuCall } from '../src/webrtc'
import { Signaling } from '../src/signaling'
import type { QosReport } from '../src/webrtc'
import type { CallState } from '../src/callRecovery'

declare global {
  interface Window {
    __dlx: {
      state: CallState
      states: CallState[]
      streams: string[]
      qos: () => Promise<QosReport | null>
      hangup: () => void
      ready: boolean
      error: string | null
      /** Estatísticas CRUAS — para diagnosticar o que o browser reporta mesmo. */
      raw: () => Promise<Record<string, unknown>[]>
      /** Envia a sugestão de camada (ver layerPolicy.ts) pelo fio, como a app faz. */
      pedirQualidade: (quality: Record<string, 'q' | 'h' | 'f'>) => void
      /** Publicadores de quem estamos a receber media. */
      publicadores: () => string[]
      /** Liga/desliga a gravação no SERVIDOR (só o anfitrião pode). */
      gravar: (on: boolean) => void
      /** Chamado quando o nó avisa que vai fechar (ver ServerMsg::Draining). */
      aoDrenar: ((reconnectInMs: number) => void) | null
    }
  }
}

const params = new URLSearchParams(location.search)
const roomToken = params.get('token') ?? ''
const code = params.get('code') ?? ''
const log = (m: string) => {
  const el = document.getElementById('log')!
  el.textContent = `${el.textContent}\n${m}`
}

window.__dlx = {
  state: 'connecting',
  states: [],
  streams: [],
  qos: async () => null,
  hangup: () => {},
  ready: false,
  error: null,
  raw: async () => [],
  pedirQualidade: () => {},
  publicadores: () => [],
  gravar: () => {},
  aoDrenar: null,
}

async function main() {
  // Media falsa do Chromium (`--use-fake-device-for-media-stream`): sinal
  // sintético determinista, que é o que permite comparar duas execuções.
  const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: true })
  const rtcConfig: RTCConfiguration = await fetch('/api/ice', {
    headers: { Authorization: `Bearer ${params.get('access') ?? ''}` },
  })
    .then((r) => (r.ok ? r.json() : { iceServers: [] }))
    .catch(() => ({ iceServers: [] }))

  const signal = new Signaling(roomToken, code)
  const call = new SfuCall(signal, stream, rtcConfig, {
    onStream: (peerId) => {
      if (!window.__dlx.streams.includes(peerId)) {
        window.__dlx.streams.push(peerId)
        log(`stream de ${peerId}`)
      }
    },
    onPeerLeft: (peerId) => {
      window.__dlx.streams = window.__dlx.streams.filter((p) => p !== peerId)
      log(`saiu ${peerId}`)
    },
    onState: (s) => {
      window.__dlx.state = s
      window.__dlx.states.push(s)
      log(`estado: ${s}`)
    },
  })

  window.__dlx.qos = () => call.qos()
  window.__dlx.pedirQualidade = (quality) => {
    signal.send({ type: 'video-interest', peers: Object.keys(quality), quality })
    log(`pedida qualidade: ${JSON.stringify(quality)}`)
  }
  signal.on('draining', ({ reconnect_in_ms }) => {
    log(`nó a drenar — migrar em ${reconnect_in_ms} ms`)
    window.__dlx.aoDrenar?.(reconnect_in_ms)
  })
  window.__dlx.gravar = (on) => {
    signal.send({ type: 'server-record', active: on })
    log(`gravação no servidor: ${on ? 'ligada' : 'desligada'}`)
  }
  window.__dlx.publicadores = () => window.__dlx.streams.filter((s) => !s.endsWith('-screen'))
  window.__dlx.raw = async () => {
    const pc = (call as unknown as { pc: RTCPeerConnection }).pc
    const out: Record<string, unknown>[] = []
    ;(await pc.getStats()).forEach((v) => out.push(v as unknown as Record<string, unknown>))
    return out
  }
  window.__dlx.hangup = () => call.hangup()
  window.__dlx.ready = true
  log('arnês pronto')
}

main().catch((e) => {
  window.__dlx.error = String(e)
  log(`ERRO: ${e}`)
})
