/**
 * O RECUO do transcritor, observado pelo motor que ARRANCA.
 *
 * PORQUE EXISTE: a bateria do `legendasPref` prova a DECISÃO (uma função pura)
 * e os invariantes do estúdio provam o TEXTO do código (`toContain`). Nenhuma
 * das duas apanhava o defeito desta sessão, que era o caminho: o `/asr` a
 * falhar a meio e o `Transcriber` a instanciar o reconhecedor da Google.
 *
 * A observação aqui é «quem foi construído» — `new webkitSpeechRecognition()`
 * contra `new Worker(whisperWorker)` — e não o valor que o `start()` devolve
 * nem a mensagem que aparece no ecrã. É a diferença entre provar que o áudio
 * NÃO sai e acreditar que não sai.
 *
 * Corre em `node` (sem jsdom): os globais do browser são esboços nossos, e têm
 * de existir ANTES do import do `media.ts` — daí o `await import()`.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

const guardado = new Map<string, string>()

/** Quantos reconhecedores da Google foram construídos. É A medida desta bateria. */
let recsConstruidos = 0
/** Quantos workers do Whisper WASM foram construídos. */
let workersConstruidos = 0
/** Os WebSockets do `/asr` abertos, para lhes provocar a falha. */
let wsAbertos: FakeWs[] = []
/** O modelo WASM está servido por esta «instalação»? (a sonda HEAD) */
let modeloServido = true

class FakeRec {
  continuous = false
  interimResults = false
  lang = ''
  onresult: ((e: unknown) => void) | null = null
  onend: (() => void) | null = null
  onerror: ((e: unknown) => void) | null = null
  constructor() {
    recsConstruidos++
  }
  start() {}
  stop() {}
}

class FakeWs {
  static OPEN = 1
  readyState = 0
  binaryType = ''
  onopen: (() => void) | null = null
  onmessage: ((e: unknown) => void) | null = null
  onerror: (() => void) | null = null
  onclose: ((e: { code: number }) => void) | null = null
  constructor(readonly url: string) {
    wsAbertos.push(this)
  }
  send() {}
  close() {}
}

class FakeWorker {
  onmessage: ((e: unknown) => void) | null = null
  constructor() {
    workersConstruidos++
  }
  postMessage() {}
  terminate() {}
}

class FakeAudioContext {
  destination = {}
  constructor(readonly opts?: unknown) {}
  createMediaStreamSource() {
    return { connect() {}, disconnect() {} }
  }
  createScriptProcessor() {
    return { onaudioprocess: null, connect() {}, disconnect() {} }
  }
  close() {
    return Promise.resolve()
  }
}

const faixaFalsa = { stop() {}, kind: 'audio' }
const streamFalso = {
  getAudioTracks: () => [faixaFalsa],
  getTracks: () => [faixaFalsa],
} as unknown as MediaStream

vi.stubGlobal('localStorage', {
  getItem: (k: string) => guardado.get(k) ?? null,
  setItem: (k: string, v: string) => void guardado.set(k, v),
  removeItem: (k: string) => void guardado.delete(k),
  clear: () => guardado.clear(),
})
vi.stubGlobal('window', { webkitSpeechRecognition: FakeRec })
vi.stubGlobal('location', { protocol: 'http:', host: 'exemplo.test' })
vi.stubGlobal('navigator', { mediaDevices: { getUserMedia: () => Promise.resolve(streamFalso) } })
vi.stubGlobal('WebSocket', FakeWs)
vi.stubGlobal('Worker', FakeWorker)
vi.stubGlobal('AudioContext', FakeAudioContext)
// A sonda do modelo: `content-type` que não seja `text/html` significa que o
// ficheiro existe (ver `whisperModelo.tamanhoEstatico`).
vi.stubGlobal('fetch', () =>
  Promise.resolve({
    ok: modeloServido,
    headers: {
      get: (h: string) =>
        h === 'content-type' ? (modeloServido ? 'application/octet-stream' : 'text/html') : '1024',
    },
  }),
)

const { Transcriber } = await import('./media')
const { esqueceMedicao } = await import('./whisperModelo')

beforeEach(() => {
  guardado.clear()
  recsConstruidos = 0
  workersConstruidos = 0
  wsAbertos = []
  modeloServido = true
  esqueceMedicao()
})

/** Arranca o transcritor e deixa o ASR do servidor FALHAR, como em produção. */
function arrancaEDeixaOAsrCair(tr: InstanceType<typeof Transcriber>) {
  tr.start('pt-PT', streamFalso)
  expect(tr.motorActivo()).toBe('servidor')
  expect(wsAbertos).toHaveLength(1)
  // O que o pod do ASR a reiniciar entrega ao browser (ver `ServerWhisperEngine`).
  wsAbertos[0].onerror?.()
}

describe('o ASR do servidor cai a meio da reunião', () => {
  it('NÃO constrói o reconhecedor da Google — arranca o Whisper local', async () => {
    // ESTE É O DEFEITO DESTA SESSÃO, medido a 2026-10-18 no commit 049948388:
    // em Chrome, uma falha do `/asr` punha o áudio a caminho da Google no meio
    // de uma reunião que começara soberana.
    const tr = new Transcriber()
    arrancaEDeixaOAsrCair(tr)
    await vi.waitFor(() => expect(workersConstruidos).toBe(1))
    expect(recsConstruidos).toBe(0)
    expect(tr.motorActivo()).toBe('wasm')
    tr.stop()
  })

  it('com a aceitação explícita, a Web Speech É alcançada', async () => {
    // O par do teste acima: sem este, a «escolha explícita» não escolheria
    // nada e a opção no ecrã seria uma mentira.
    guardado.set('dx_asr_webspeech', '1')
    const tr = new Transcriber()
    arrancaEDeixaOAsrCair(tr)
    await vi.waitFor(() => expect(recsConstruidos).toBe(1))
    expect(workersConstruidos).toBe(0)
    expect(tr.motorActivo()).toBe('webspeech')
    tr.stop()
  })

  it('a organização a proibir vence a aceitação guardada no browser', async () => {
    // A preferência do cliente deixa de ser a única guarda: o `localStorage`
    // diz «aceito» (podia tê-lo escrito uma extensão) e o servidor diz «não».
    guardado.set('dx_asr_webspeech', '1')
    const tr = new Transcriber()
    tr.politicaDaOrganizacao(false)
    arrancaEDeixaOAsrCair(tr)
    await vi.waitFor(() => expect(workersConstruidos).toBe(1))
    expect(recsConstruidos).toBe(0)
    expect(tr.motorActivo()).toBe('wasm')
    tr.stop()
  })

  it('sem modelo local servido e sem aceitação: DIZ que não há motor', async () => {
    // O caminho que antes acabava na Google em silêncio. A mensagem nomeia a
    // saída (o ecrã de Inteligência) e o custo dela (o áudio vai à Google).
    modeloServido = false
    const tr = new Transcriber()
    const erros: string[] = []
    tr.onError = (m) => erros.push(m)
    arrancaEDeixaOAsrCair(tr)
    await vi.waitFor(() => expect(erros).toHaveLength(1))
    expect(recsConstruidos).toBe(0)
    expect(erros[0]).toMatch(/Google/)
    expect(tr.motorActivo()).toBe('nenhum')
    tr.stop()
  })
})

describe('a política da organização a chegar com o motor já a correr', () => {
  it('pára o reconhecedor da Google e passa ao Whisper local', async () => {
    // A recusa de uma frase pelo servidor traz a política outra vez (ver
    // `signaling.rs`), e é por aqui que ela alcança quem já estava na sala
    // quando a organização mudou de ideias.
    guardado.set('dx_asr_webspeech', '1')
    const tr = new Transcriber()
    arrancaEDeixaOAsrCair(tr)
    await vi.waitFor(() => expect(recsConstruidos).toBe(1))
    tr.politicaDaOrganizacao(false)
    await vi.waitFor(() => expect(workersConstruidos).toBe(1))
    expect(tr.motorActivo()).toBe('wasm')
    tr.stop()
  })
})
