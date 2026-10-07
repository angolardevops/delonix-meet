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
import { LinhaDoTempo } from '../src/callTimings'

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
      /** Grava a media LOCAL com o `MediaRecorder` do browser durante `ms` e
       *  devolve o webm em base64 — a régua a medir-se a si própria. */
      gravarLocal: (ms: number) => Promise<string>
      /** Chamado quando o nó avisa que vai fechar (ver ServerMsg::Draining). */
      aoDrenar: ((reconnectInMs: number) => void) | null
      /** Linha do tempo desta sessão (ver callTimings.ts). */
      tempos: LinhaDoTempo | null
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
  gravarLocal: async () => '',
  aoDrenar: null,
  tempos: null,
}

/** Período da régua, em segundos: um clarão e um toque no mesmo instante. */
const REGUA_PERIODO = 2
const REGUA_MARCA = 0.1

/** `fonte=regua`: som e imagem de REFERÊNCIA, para medir o sincronismo de uma
 *  gravação. A câmara e o microfone falsos do Chromium (de ficheiro ou não)
 *  arrancam cada um por si, e a distância entre os dois muda a cada entrada —
 *  não servem de régua. Aqui os dois saem do MESMO relógio (o do
 *  `AudioContext`): de dois em dois segundos, 100 ms de ecrã branco e 100 ms de
 *  tom a `tom` Hz.
 *
 *  `fundo`: um tom que oscila entre os toques. Sem ele o microfone está em
 *  silêncio digital e o DTX cala o envio (um pacote a cada 400 ms).
 *  `faixa`: uma faixa clara fixa no topo da imagem — numa grelha, diz de quem
 *  é cada célula sem depender dos clarões. */
async function regua(fundo: boolean, tom: number, faixa: boolean): Promise<MediaStream> {
  const ctx = new AudioContext({ sampleRate: 48000 })
  await ctx.resume()
  const saida = ctx.createMediaStreamDestination()
  // Uma fonte sempre viva, a zeros: sem ela o grafo pára entre os toques, a
  // pista deixa de receber amostras e o relógio RTP não anda — um microfone a
  // sério entrega silêncio, não entrega nada.
  const viva = ctx.createConstantSource()
  viva.offset.value = 1e-5
  viva.connect(saida)
  viva.start()
  if (fundo) {
    const cama = ctx.createOscillator()
    const vibrato = ctx.createOscillator()
    const fundura = ctx.createGain()
    const volume = ctx.createGain()
    cama.frequency.value = 400
    vibrato.frequency.value = 3
    fundura.gain.value = 150
    volume.gain.value = 0.08
    vibrato.connect(fundura).connect(cama.frequency)
    cama.connect(volume).connect(saida)
    cama.start()
    vibrato.start()
  }
  // Os toques marcam-se no relógio do áudio, com meio segundo de avanço.
  let marcado = Math.ceil(ctx.currentTime / REGUA_PERIODO) * REGUA_PERIODO
  const marca = () => {
    while (marcado < ctx.currentTime + 0.5) {
      const o = ctx.createOscillator()
      const g = ctx.createGain()
      o.frequency.value = tom
      g.gain.value = 0.6
      o.connect(g).connect(saida)
      o.start(marcado)
      o.stop(marcado + REGUA_MARCA)
      marcado += REGUA_PERIODO
    }
  }
  marca()
  setInterval(marca, 100)

  const tela = document.createElement('canvas')
  tela.width = 640
  tela.height = 480
  const c = tela.getContext('2d')!
  const pinta = () => {
    // O instante que se OUVE agora: o relógio do contexto vai adiantado à saída.
    const t = ctx.currentTime - (ctx.baseLatency || 0)
    const fase = ((t % REGUA_PERIODO) + REGUA_PERIODO) % REGUA_PERIODO
    c.fillStyle = fase < REGUA_MARCA ? '#fff' : '#222'
    c.fillRect(0, 0, 640, 480)
    // Um traço que anda: a tela só entrega quadros quando muda.
    c.fillStyle = '#777'
    c.fillRect((fase / REGUA_PERIODO) * 624, 464, 16, 16)
    if (faixa) {
      c.fillStyle = '#999'
      c.fillRect(0, 0, 640, 120)
    }
    requestAnimationFrame(pinta)
  }
  pinta()
  return new MediaStream([...tela.captureStream(30).getVideoTracks(), ...saida.stream.getAudioTracks()])
}

async function main() {
  // Media falsa do Chromium (`--use-fake-device-for-media-stream`): sinal
  // sintético determinista, que é o que permite comparar duas execuções.
  // A linha do tempo começa antes de haver PC — é o tempo do UTILIZADOR.
  const tempos = new LinhaDoTempo()
  tempos.marcar('intencao')
  window.__dlx.tempos = tempos
  // `som=cru` tira o cancelamento de eco, a supressão de ruído e o ganho
  // automático: quem mede um TOM do outro lado precisa de que ele lá chegue, e
  // a supressão de ruído trata um tom constante como ruído.
  const audio: boolean | MediaTrackConstraints =
    params.get('som') === 'cru'
      ? { echoCancellation: false, noiseSuppression: false, autoGainControl: false }
      : true
  const fonte = params.get('fonte')
  const stream =
    fonte === 'nada'
      ? // Só assiste: entra sem publicar nada (subscreve os outros, e mais nada).
        new MediaStream()
      : fonte === 'regua'
      ? await regua(params.get('fundo') === '1', Number(params.get('tom')) || 2500, params.get('faixa') === '1')
      : await navigator.mediaDevices.getUserMedia({ audio, video: true })
  const rtcConfig: RTCConfiguration = await fetch('/api/ice-servers', {
    headers: { Authorization: `Bearer ${params.get('access') ?? ''}` },
  })
    .then((r) => (r.ok ? r.json() : { iceServers: [] }))
    .catch(() => ({ iceServers: [] }))

  tempos.marcar('token')
  const signal = new Signaling(roomToken, code)
  signal.on('joined', () => tempos.marcar('ws'))
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
  }, undefined, undefined, tempos)

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
  window.__dlx.gravarLocal = (ms) =>
    new Promise((resolve, reject) => {
      const rec = new MediaRecorder(stream, { mimeType: 'video/webm;codecs=vp8,opus' })
      const partes: Blob[] = []
      rec.ondataavailable = (e) => partes.push(e.data)
      rec.onerror = () => reject(new Error('MediaRecorder falhou'))
      rec.onstop = async () => {
        const b = new Uint8Array(await new Blob(partes).arrayBuffer())
        let s = ''
        for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000))
        resolve(btoa(s))
      }
      rec.start()
      setTimeout(() => rec.stop(), ms)
    })
  window.__dlx.ready = true
  log('arnês pronto')
}

main().catch((e) => {
  window.__dlx.error = String(e)
  log(`ERRO: ${e}`)
})
