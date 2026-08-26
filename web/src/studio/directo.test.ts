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
    const u = urlDoDirecto({ protocol: 'https:', host: 'meet.exemplo' }, 'sala-azul', 't', destino)
    expect(u.startsWith('wss://meet.exemplo/')).toBe(true)
  })

  it('e ws quando é http (rede interna)', () => {
    const u = urlDoDirecto({ protocol: 'http:', host: 'localhost:5173' }, 'sala-azul', 't', destino)
    expect(u.startsWith('ws://localhost:5173/')).toBe(true)
  })

  it('leva o token, o destino, a chave e o codec', () => {
    const q = new URL(urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 'tok', destino)).searchParams
    expect(q.get('token')).toBe('tok')
    expect(q.get('destino')).toBe(destino.url)
    expect(q.get('chave')).toBe('k-123')
    // O codec vai declarado para o servidor poder RECUSAR antes de gastar um
    // processo de ffmpeg.
    expect(q.get('codec')).toBe('video/h264')
  })

  it('escapa o código da sala', () => {
    const u = urlDoDirecto({ protocol: 'https:', host: 'h' }, 'a/b?c', 't', destino)
    expect(u).toContain('/api/rooms/a%2Fb%3Fc/broadcast')
  })

  it('apara espaços à volta do destino e da chave', () => {
    // Colar uma chave de uma página web traz espaços, e um URL com espaço no
    // fim dá um erro do ffmpeg que ninguém liga à causa.
    const q = new URL(
      urlDoDirecto({ protocol: 'https:', host: 'h' }, 'c', 't', {
        url: '  rtmp://x/live  ',
        chave: '  k  ',
      }),
    ).searchParams
    expect(q.get('destino')).toBe('rtmp://x/live')
    expect(q.get('chave')).toBe('k')
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
