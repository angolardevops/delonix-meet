/**
 * O contrato do directo com o servidor.
 *
 * O comportamento com media a sério vive no e2e; isto guarda as decisões que
 * um `git revert` distraído desfaz — e a mais importante é o CODEC: se este
 * módulo enviar VP8, o servidor tem de reencodificar e a decisão inteira do
 * ADR-0003 cai por terra.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { CODEC_DIRECTO, MIME_DIRECTO, urlDoDirecto } from './directo'

const raiz = join(__dirname, '..', '..', '..')
const ler = (p: string) => readFileSync(join(raiz, p), 'utf8')

describe('o codec é um contrato, não uma preferência', () => {
  it('o browser emite H.264', () => {
    // Sem H.264 o servidor não pode fazer `-c:v copy`, e um encode de vídeo no
    // pod satura o core que serve a chamada que está a ser emitida.
    expect(MIME_DIRECTO).toContain('h264')
    expect(CODEC_DIRECTO).toBe('video/h264')
  })

  it('e o servidor só aceita o que o browser diz enviar', () => {
    // As duas pontas têm de concordar. Se uma mudar sozinha, o directo é
    // recusado (bom) ou aceite e produz lixo (mau) — depende de qual mudou.
    const rs = ler('server/src/broadcast.rs')
    expect(rs).toContain('"video/h264" | "video/avc"')
  })

  it('o servidor desmultiplexa Matroska, que é o que o MediaRecorder produz', () => {
    // Medido (R76): `video/webm;codecs=h264` sai como `matroska,webm`. O WebM
    // oficialmente não admite H.264; `-f webm` funcionava por sorte.
    const rs = ler('server/src/broadcast.rs')
    expect(rs).toContain('"matroska".into()')
    expect(rs).not.toMatch(/"-f"\.into\(\),\s*"webm"\.into\(\)/)
  })
})

describe('urlDoDirecto', () => {
  const destino = { url: 'rtmp://a.rtmp.youtube.com/live2', chave: 'k-123', rotulo: 'YouTube' }

  it('usa wss quando a página é https', () => {
    const u = urlDoDirecto({ protocol: 'https:', host: 'meet.exemplo' }, 'sala-azul', 't', [destino])
    expect(u.startsWith('wss://meet.exemplo/')).toBe(true)
  })

  it('e ws quando é http (rede interna)', () => {
    const u = urlDoDirecto({ protocol: 'http:', host: 'localhost:5173' }, 'sala-azul', 't', [destino])
    expect(u.startsWith('ws://localhost:5173/')).toBe(true)
  })

  it('leva o token, os destinos (em JSON) e o codec', () => {
    const q = new URL(urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 'tok', [destino])).searchParams
    expect(q.get('token')).toBe('tok')
    expect(JSON.parse(q.get('destinos')!)).toEqual([destino])
    // O codec vai declarado para o servidor poder RECUSAR antes de gastar um
    // processo de ffmpeg.
    expect(q.get('codec')).toBe('video/h264')
  })

  it('escapa o código da sala', () => {
    const u = urlDoDirecto({ protocol: 'https:', host: 'h' }, 'a/b?c', 't', [destino])
    expect(u).toContain('/api/rooms/a%2Fb%3Fc/live')
  })

  it('apara espaços à volta do url e da chave de cada destino', () => {
    // Colar uma chave de uma página web traz espaços, e um URL com espaço no
    // fim dá um erro do ffmpeg que ninguém liga à causa.
    const q = new URL(
      urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 't', [
        { url: '  rtmp://x/live  ', chave: '  k  ' },
      ]),
    ).searchParams
    expect(JSON.parse(q.get('destinos')!)).toEqual([{ url: 'rtmp://x/live', chave: 'k' }])
  })

  it('leva VÁRIOS destinos — o multi-canal tipo StreamYard', () => {
    // É a capacidade inteira desta mudança: um array, não um campo.
    const outro = { url: 'rtmp://live.twitch.tv/app', chave: 'k-456', rotulo: 'Twitch' }
    const q = new URL(urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 't', [destino, outro])).searchParams
    expect(JSON.parse(q.get('destinos')!)).toEqual([destino, outro])
  })

  it('um destino sem rótulo não leva a chave "rotulo" no JSON', () => {
    // O servidor já tem uma predefinição («directo») para quando falta — não
    // se manda `rotulo: undefined`, que o `JSON.stringify` normal omitiria de
    // qualquer forma, mas é o contrato que se quer garantido, não um acaso.
    const q = new URL(
      urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 't', [{ url: 'rtmp://x', chave: 'k' }]),
    ).searchParams
    const brutos = JSON.parse(q.get('destinos')!) as Record<string, unknown>[]
    expect('rotulo' in brutos[0]).toBe(false)
  })
})

describe('o fluxo composto é partilhado, não duplicado', () => {
  const c = () =>
    ler('web/src/studio/compositor.ts')
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .split('\n')
      .filter((l) => !l.trimStart().startsWith('//'))
      .join('\n')

  it('há UM sítio que monta o fluxo', () => {
    // A gravação e o directo precisam do mesmo. Duas montagens divergiriam à
    // primeira correcção feita só numa — e o áudio é onde isso doeria.
    expect(c()).toContain('async montarFluxo(')
    expect(c().match(/createMediaStreamDestination\(\)/g)?.length).toBe(1)
  })

  it('e só se desmonta quando o ÚLTIMO consumidor larga', () => {
    // Parar a gravação FECHAVA o AudioContext, e um directo a decorrer sobre o
    // mesmo fluxo emudecia nesse instante, sem erro nenhum.
    expect(c()).toContain('this.consumidores++')
    expect(c()).toContain('if (this.consumidores > 0) return')
    // A desmontagem tem de estar SÓ no `largarFluxo`, não repetida no fim da
    // gravação — repetida, a contagem não serviria de nada.
    expect(c().match(/this\.audioCtx\?\.close\(\)/g)?.length).toBe(2) // largarFluxo + destruir
  })
})

// ── A classe `Directo` — o que o arnês de mutação encontrou sem defesa ──────
//
// Sete das oito mutações a este ficheiro SOBREVIVIAM: o suporte do browser, a
// guarda de pedaço vazio, a de socket fechado, e as três do estado. Testes de
// contrato (codec, URL) não cobrem o ciclo de vida, e é no ciclo de vida que
// este módulo pode falhar em silêncio — enviar num socket fechado atira, e
// enviar um pedaço vazio é largura de banda a troco de nada.
//
// Nada disto precisa de rede nem de câmara: substituem-se o `MediaRecorder` e o
// `WebSocket` por duplos com a mesma forma. O que se prova são as DECISÕES.
import { Directo, directoSuportado } from './directo'

class SocketFalso {
  static ABERTO = 1
  readyState = 1
  binaryType = ''
  enviados: unknown[] = []
  onopen: (() => void) | null = null
  onclose: ((e?: unknown) => void) | null = null
  onerror: (() => void) | null = null
  onmessage: ((e: { data: unknown }) => void) | null = null
  fechado: [number, string] | null = null
  constructor(public url: string) {
    queueMicrotask(() => this.onopen?.())
  }
  send(b: unknown) {
    if (this.readyState !== 1) throw new Error('send num socket fechado')
    this.enviados.push(b)
  }
  close(c: number, r: string) {
    this.fechado = [c, r]
    this.readyState = 3
  }
}

class GravadorFalso {
  static suportado = true
  static isTypeSupported() {
    return GravadorFalso.suportado
  }
  state = 'inactive'
  ondataavailable: ((e: { data: { size: number; arrayBuffer: () => Promise<ArrayBuffer> } }) => void) | null = null
  onstop: (() => void) | null = null
  constructor(_s: unknown, public opcoes: { mimeType: string; videoBitsPerSecond: number }) {}
  start(_fatia: number) {
    this.state = 'recording'
  }
  stop() {
    this.state = 'inactive'
    queueMicrotask(() => this.onstop?.())
  }
}

const pedaco = (size: number) => ({ data: { size, arrayBuffer: async () => new ArrayBuffer(size) } })
const esperar = () => new Promise((r) => setTimeout(r, 300))

function montarAmbiente() {
  const criados: SocketFalso[] = []
  const g = globalThis as unknown as Record<string, unknown>
  g.WebSocket = class extends SocketFalso {
    static OPEN = 1
    constructor(url: string) {
      super(url)
      criados.push(this)
    }
  }
  ;(g.WebSocket as unknown as { OPEN: number }).OPEN = 1
  g.MediaRecorder = GravadorFalso
  g.location = { protocol: 'https:', host: 'meet.teste' }
  GravadorFalso.suportado = true
  return { criados }
}

describe('Directo · o browser tem de saber codificar H.264', () => {
  it('sem MediaRecorder não há directo', () => {
    const g = globalThis as unknown as Record<string, unknown>
    const antes = g.MediaRecorder
    delete g.MediaRecorder
    expect(directoSuportado()).toBe(false)
    g.MediaRecorder = antes
  })

  it('com MediaRecorder mas SEM H.264 também não', () => {
    // O `&&` aqui não é decoração: um browser com MediaRecorder e sem H.264 é
    // o caso comum (Firefox), e deixá-lo arrancar dava um directo que o
    // servidor teria de reencodificar — a decisão que o ADR-0003 recusou.
    montarAmbiente()
    GravadorFalso.suportado = false
    expect(directoSuportado()).toBe(false)
    GravadorFalso.suportado = true
    expect(directoSuportado()).toBe(true)
  })
})

describe('Directo · o ciclo de vida', () => {
  it('vai ao ar, envia pedaços, e conta os bytes', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    const fases: string[] = []
    d.aoMudar = (e) => fases.push(e.fase)
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    expect(fases).toEqual(['a-ligar', 'no-ar'])
    expect(d.noAr).toBe(true)

    const rec = (d as unknown as { gravador: GravadorFalso }).gravador
    rec.ondataavailable?.(pedaco(1000))
    await esperar()
    expect(criados[0].enviados).toHaveLength(1)
    expect(d.estado).toMatchObject({ fase: 'no-ar', bytes: 1000 })
  })

  it('um pedaço VAZIO não vai para a rede', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    const rec = (d as unknown as { gravador: GravadorFalso }).gravador
    rec.ondataavailable?.(pedaco(0))
    await esperar()
    expect(criados[0].enviados).toHaveLength(0)
    expect(d.estado).toMatchObject({ bytes: 0 })
  })

  it('com o socket já fechado, não se envia — nem se contam bytes', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    criados[0].readyState = 3
    const rec = (d as unknown as { gravador: GravadorFalso }).gravador
    rec.ondataavailable?.(pedaco(500))
    await esperar()
    expect(criados[0].enviados).toHaveLength(0)
    expect(d.estado).toMatchObject({ bytes: 0 })
  })

  it('e se fechar ENTRE o pedaço e o `arrayBuffer`, o envio é abandonado', async () => {
    // A guarda dupla existe porque o `arrayBuffer()` é assíncrono: o socket
    // pode fechar no meio. Sem a segunda, o `send` atira num socket fechado.
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    const rec = (d as unknown as { gravador: GravadorFalso }).gravador
    rec.ondataavailable?.({
      data: {
        size: 500,
        arrayBuffer: async () => {
          criados[0].readyState = 3
          return new ArrayBuffer(500)
        },
      },
    })
    await esperar()
    expect(criados[0].enviados).toHaveLength(0)
    // Os bytes CONTAM (o pedaço existiu), mas não foram enviados.
    expect(d.estado).toMatchObject({ bytes: 500 })
  })

  it('parar fecha o socket com 1000 e volta a «parado»', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    await d.parar()
    expect(criados[0].fechado).toEqual([1000, 'fim'])
    expect(d.estado).toEqual({ fase: 'parado' })
    expect(d.noAr).toBe(false)
  })

  it('parar sem nunca ter começado não atira', async () => {
    // O `g &&` não é defensivo por hábito: sem ele, `parar()` numa emissão que
    // nunca arrancou lê `.state` de `null` e atira um TypeError — dentro de um
    // `onClick`, que é onde um erro não tratado passa despercebido até alguém
    // abrir a consola.
    montarAmbiente()
    const d = new Directo()
    await expect(d.parar()).resolves.toBeUndefined()
    expect(d.estado).toEqual({ fase: 'parado' })
  })

  it('o socket a cair leva a «erro», não a «parado»', async () => {
    // A distinção importa na interface: «parado» foi decisão da pessoa, «erro»
    // é uma emissão que caiu e que ela tem de saber que caiu.
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    criados[0].onclose?.()
    expect(d.estado).toMatchObject({ fase: 'erro' })
    expect(d.noAr).toBe(false)
  })

  it('e depois de PARADO, um fecho tardio não põe «erro» no ecrã', async () => {
    // O `parar()` fecha o socket, e o fecho chega DEPOIS. Sem a guarda de
    // fase, a interface mostrava um erro logo a seguir a a pessoa ter parado
    // de propósito.
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    await d.parar()
    criados[0].onclose?.()
    expect(d.estado).toEqual({ fase: 'parado' })
  })
})

// ── Estado por destino (frontend/b1-emissao) ────────────────────────────────
//
// O servidor passou a ter um processo por destino e a mandar o estado de cada
// um. O contrato de nomes está do lado de lá em `DestinationReport`
// (`server/src/broadcast.rs`); aqui guarda-se a leitura, que tem de ser
// tolerante: um servidor mais novo não pode partir um cliente mais velho, nem
// um destino malformado esconder os outros.
import { lerMensagemDoDirecto, resumirDestinos } from './directo'

const destinoDoServidor = {
  dest: 0,
  rotulo: 'YouTube',
  platform: 'youtube',
  estado: 'no-ar',
  kbps: 4500,
  perdas: 0.2,
  motivo: null,
  tentativas: 0,
  frames_descartados: 3,
  bytes_enviados: 1024,
}

describe('lerMensagemDoDirecto', () => {
  it('lê o estado dos destinos e passa os nomes do servidor para os do cliente', () => {
    const m = lerMensagemDoDirecto(
      JSON.stringify({ tipo: 'destinos', destinos: [{ ...destinoDoServidor, id: 'abc' }] }),
    )
    expect(m).toEqual({
      tipo: 'destinos',
      destinos: [
        {
          dest: 0,
          id: 'abc',
          rotulo: 'YouTube',
          platform: 'youtube',
          estado: 'no-ar',
          kbps: 4500,
          perdas: 0.2,
          motivo: null,
          tentativas: 0,
          framesDescartados: 3,
          bytesEnviados: 1024,
        },
      ],
    })
  })

  it('lê a recusa/fim do servidor', () => {
    expect(lerMensagemDoDirecto('{"erro":"nenhum destino ficou no ar"}')).toEqual({
      tipo: 'erro',
      motivo: 'nenhum destino ficou no ar',
    })
  })

  it('ignora o que não conhece em vez de atirar', () => {
    expect(lerMensagemDoDirecto('não é json')).toBeNull()
    expect(lerMensagemDoDirecto('null')).toBeNull()
    expect(lerMensagemDoDirecto('{"tipo":"espectadores","n":12}')).toBeNull()
    expect(lerMensagemDoDirecto('{"tipo":"destinos","destinos":"x"}')).toBeNull()
  })

  it('um destino malformado sai sozinho; os outros ficam', () => {
    const m = lerMensagemDoDirecto(
      JSON.stringify({
        tipo: 'destinos',
        destinos: [
          destinoDoServidor,
          { ...destinoDoServidor, dest: 1, estado: 'desconhecido' },
          { ...destinoDoServidor, dest: -1 },
          'lixo',
          { ...destinoDoServidor, dest: 2, estado: 'erro', motivo: 'recusou', kbps: -5, perdas: 'x' },
        ],
      }),
    )
    expect(m?.tipo).toBe('destinos')
    const d = (m as { destinos: { dest: number; kbps: number; perdas: number; motivo: string | null }[] }).destinos
    expect(d.map((x) => x.dest)).toEqual([0, 2])
    expect(d[1]).toMatchObject({ kbps: 0, perdas: 0, motivo: 'recusou' })
  })

  it('um motivo vazio é «sem motivo»', () => {
    const m = lerMensagemDoDirecto(
      JSON.stringify({ tipo: 'destinos', destinos: [{ ...destinoDoServidor, motivo: '' }] }),
    )
    expect(m).toMatchObject({ destinos: [{ motivo: null }] })
  })
})

describe('resumirDestinos', () => {
  it('conta por estado e soma o débito só de quem está no ar', () => {
    const base = lerMensagemDoDirecto(JSON.stringify({ tipo: 'destinos', destinos: [destinoDoServidor] }))
    const um = (base as { destinos: Parameters<typeof resumirDestinos>[0] }).destinos[0]
    const r = resumirDestinos([
      um,
      { ...um, dest: 1, kbps: 3000 },
      { ...um, dest: 2, estado: 'parado', kbps: 999 },
      { ...um, dest: 3, estado: 'erro' },
    ])
    expect(r).toEqual({ 'a-ligar': 0, 'no-ar': 2, parado: 1, erro: 1, total: 4, kbps: 7500 })
  })
})

describe('urlDoDirecto · destinos guardados', () => {
  it('um destino guardado vai SÓ pelo id: nem URL nem chave saem do browser', () => {
    const q = new URL(
      urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 't', [
        { id: 'd-1', url: 'rtmp://nao-vai', chave: 'nao-vai', rotulo: 'Canal' },
        { url: 'rtmp://x/live', chave: 'k' },
      ]),
    ).searchParams
    expect(JSON.parse(q.get('destinos')!)).toEqual([{ id: 'd-1', rotulo: 'Canal' }, { url: 'rtmp://x/live', chave: 'k' }])
    expect(q.get('destinos')).not.toContain('nao-vai')
  })
})

describe('Directo · estado por destino', () => {
  const estadoTexto = (destinos: unknown[]) => ({ data: JSON.stringify({ tipo: 'destinos', destinos }) })

  it('guarda e anuncia o estado dos destinos que o servidor manda', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    const vistos: number[] = []
    d.aoMudarDestinos = (x) => vistos.push(x.length)
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    criados[0].onmessage?.(estadoTexto([destinoDoServidor, { ...destinoDoServidor, dest: 1, estado: 'erro' }]))
    expect(vistos).toEqual([2])
    expect(d.destinos.map((x) => x.estado)).toEqual(['no-ar', 'erro'])
    // Uma trama binária ou desconhecida não mexe no que se sabe.
    criados[0].onmessage?.({ data: new ArrayBuffer(1) })
    criados[0].onmessage?.({ data: '{"tipo":"outra-coisa"}' })
    expect(d.destinos).toHaveLength(2)
  })

  it('o estado a chegar durante o aceite também conta', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    const p = d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    await Promise.resolve()
    criados[0].onmessage?.(estadoTexto([destinoDoServidor]))
    await p
    expect(d.noAr).toBe(true)
    expect(d.destinos).toHaveLength(1)
  })

  it('um fim pedido pelo servidor mostra a razão dele, não «caiu»', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    criados[0].onmessage?.({ data: '{"erro":"nenhum destino ficou no ar — YouTube: chave errada"}' })
    criados[0].onclose?.()
    expect(d.estado).toEqual({ fase: 'erro', motivo: 'nenhum destino ficou no ar — YouTube: chave errada' })
  })

  it('começar de novo limpa o estado da emissão anterior', async () => {
    const { criados } = montarAmbiente()
    const d = new Directo()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    criados[0].onmessage?.(estadoTexto([destinoDoServidor]))
    criados[0].onmessage?.({ data: '{"erro":"x"}' })
    await d.parar()
    await d.comecar({} as MediaStream, 'sala', 'tok', [{ url: 'rtmp://x', chave: 'k' }])
    expect(d.destinos).toEqual([])
    criados[1].onclose?.()
    expect(d.estado).toEqual({ fase: 'erro', motivo: 'a ligação ao servidor caiu' })
  })
})
