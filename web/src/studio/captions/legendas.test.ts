/**
 * SRT/VTT, palavras → cues, enchimentos e o corte pelo texto. O que se protege:
 * um ficheiro de legendas que abre em qualquer leitor, e apagar palavras no
 * texto corta EXACTAMENTE esse trecho do vídeo — nem a cauda da palavra
 * anterior, nem um buraco de silêncio a seguir.
 */
import { describe, expect, it, vi } from 'vitest'
import type { Cue, Palavra } from '../edit/projecto'
import {
  contarPreenchimento,
  distribuirPalavras,
  editarTextoDaCue,
  encontrarPreenchimento,
  intervalosDasPalavras,
  lerLegendas,
  lerTempo,
  lerTimecode,
  palavrasParaCues,
  paraSrt,
  paraVtt,
  partirTexto,
  tempoDeLegenda,
  timecode,
  traduzirCues,
} from './legendas'

const w = (inicio: number, fim: number, texto: string): Palavra => ({ inicio, fim, texto })

const CUES: Cue[] = [
  { id: 'a', inicio: 1.2, fim: 3.5, texto: 'O failover foi corrigido.' },
  { id: 'b', inicio: 3661.007, fim: 3662, texto: 'Linha um\nLinha dois', orador: 'Ana Mbala' },
]

describe('tempos', () => {
  it('formata SRT com vírgula e VTT com ponto, com horas', () => {
    expect(tempoDeLegenda(3661.007, ',')).toBe('01:01:01,007')
    expect(tempoDeLegenda(1.2, '.')).toBe('00:00:01.200')
  })

  it('lê as duas formas e a curta do VTT', () => {
    expect(lerTempo('01:01:01,007')).toBeCloseTo(3661.007)
    expect(lerTempo('00:01.5')).toBeCloseTo(1.5)
    expect(lerTempo('lixo')).toBeNaN()
  })

  it('timecode com frame', () => {
    expect(timecode(1102.5, 30)).toBe('00:18:22:15')
  })

  it('lê o que se escreve num campo de tempo', () => {
    expect(lerTimecode('00:18:22:15')).toBeCloseTo(1102.5)
    expect(lerTimecode('16:04')).toBe(964)
    expect(lerTimecode('01:02:03')).toBe(3723)
    expect(lerTimecode('3,5')).toBe(3.5)
    expect(lerTimecode('abc')).toBeNaN()
  })
})

describe('SRT e VTT', () => {
  it('SRT: numeração, seta, texto', () => {
    expect(paraSrt(CUES)).toBe(
      '1\n00:00:01,200 --> 00:00:03,500\nO failover foi corrigido.\n\n2\n01:01:01,007 --> 01:01:02,000\nLinha um\nLinha dois\n',
    )
  })

  it('VTT: cabeçalho obrigatório e orador como <v>', () => {
    const v = paraVtt(CUES, { oradores: true })
    expect(v.startsWith('WEBVTT\n\n')).toBe(true)
    expect(v).toContain('00:00:01.200 --> 00:00:03.500')
    expect(v).toContain('<v Ana Mbala>Linha um')
  })

  it('VTT karaoke: etiqueta de tempo antes de cada palavra a partir da segunda', () => {
    const v = paraVtt([{ id: 'k', inicio: 0, fim: 2, texto: 'olá mundo', palavras: [w(0, 0.5, 'olá'), w(0.8, 1.5, 'mundo')] }], {
      karaoke: true,
    })
    expect(v).toContain('olá <00:00:00.800>mundo')
  })

  it('uma linha em branco ou uma seta no texto não partem o ficheiro', () => {
    const s = paraSrt([{ id: 'x', inicio: 0, fim: 1, texto: 'a\n\n\nb --> c' }])
    expect(lerLegendas(s)).toHaveLength(1)
    expect(lerLegendas(s)[0].texto).toBe('a\nb → c')
  })

  it('ida e volta: o que se escreve lê-se igual', () => {
    for (const texto of [paraSrt(CUES), paraVtt(CUES, { oradores: true })]) {
      const lidas = lerLegendas(texto)
      expect(lidas.map((c) => [c.inicio, c.fim, c.texto])).toEqual(CUES.map((c) => [c.inicio, c.fim, c.texto]))
    }
    expect(lerLegendas(paraVtt(CUES, { oradores: true }))[1].orador).toBe('Ana Mbala')
  })

  it('cues vazias ou com tempo invertido não saem', () => {
    expect(paraSrt([{ id: 'x', inicio: 2, fim: 1, texto: 'a' }, { id: 'y', inicio: 0, fim: 1, texto: '  ' }])).toBe('')
  })
})

describe('palavras → cues', () => {
  it('fecha na pausa longa e na pontuação final', () => {
    const cues = palavrasParaCues([
      w(0, 0.4, 'Sim,'),
      w(0.5, 0.9, 'ficou'),
      w(1, 1.4, 'corrigido'),
      w(1.5, 1.9, 'na'),
      w(2, 2.4, 'versão'),
      w(2.5, 2.9, '2.4.'),
      w(3, 3.4, 'Obrigada'),
      w(5, 5.4, 'Perguntas?'),
    ])
    expect(cues.map((c) => c.texto)).toEqual(['Sim, ficou corrigido na versão 2.4.', 'Obrigada', 'Perguntas?'])
    expect(cues[0].palavras).toHaveLength(6)
    expect(cues[1]).toMatchObject({ inicio: 3, fim: 3.4 })
  })

  it('não passa do tamanho máximo', () => {
    const muitas = Array.from({ length: 60 }, (_, i) => w(i * 0.1, i * 0.1 + 0.08, 'palavra'))
    for (const c of palavrasParaCues(muitas)) expect(c.texto.length).toBeLessThanOrEqual(84)
  })

  it('distribui tempos proporcionais quando o modelo só dá segmentos', () => {
    const ws = distribuirPalavras('ab abcd', 10, 16)
    expect(ws[0]).toMatchObject({ inicio: 10, fim: 12 })
    expect(ws[1].fim).toBeCloseTo(16)
  })

  it('editar o texto com o mesmo número de palavras mantém os tempos medidos', () => {
    const c: Cue = { id: 'c', inicio: 0, fim: 2, texto: 'o failóver', palavras: [w(0, 0.3, 'o'), w(0.5, 1.8, 'failóver')] }
    const e = editarTextoDaCue(c, 'o failover')
    expect(e.palavras![1]).toEqual(w(0.5, 1.8, 'failover'))
  })
})

describe('palavras de preenchimento', () => {
  const ws = ['Fica,', 'sim.', 'Pá,', 'pronto,', 'mostro', 'tipo', 'no', 'quer', 'dizer', 'diapositivo', 'pá'].map((t, i) => w(i, i + 0.5, t))

  it('encontra termos simples e expressões, ignorando maiúsculas e pontuação', () => {
    const o = encontrarPreenchimento(ws, 'pt-AO')
    expect(o.map((x) => x.termo)).toEqual(['pá', 'pronto', 'tipo', 'quer dizer', 'pá'])
    expect(o[3].indices).toEqual([7, 8])
    expect(contarPreenchimento(o)[0]).toEqual({ termo: 'pá', n: 2 })
  })

  it('língua sem lista não inventa nada', () => {
    expect(encontrarPreenchimento(ws, 'umb')).toEqual([])
  })

  it('aceita termos de fora da lista (os que o LLM encontrou) sem duplicar os fixos', () => {
    const ws = ['Então', 'basicamente', 'o', 'failover,', 'tipo,', 'funciona', 'Basicamente.'].map((x, i) => w(i, i + 0.5, x))
    const o = encontrarPreenchimento(ws, 'pt', ['basicamente', 'tipo', '  '])
    expect(o.map((x) => [x.termo, x.indices])).toEqual([
      ['basicamente', [1]],
      ['tipo', [4]],
      ['basicamente', [6]],
    ])
  })
})

describe('corte pelo texto', () => {
  const ws = [w(0, 0.5, 'Fica,'), w(0.6, 1, 'sim.'), w(1.3, 1.5, 'pá,'), w(1.6, 2, 'pronto,'), w(2.4, 3, 'mostro'), w(3.1, 3.5, 'já')]

  it('apagar uma corrida corta do início da primeira ao início da que fica', () => {
    expect(intervalosDasPalavras(ws, new Set([2, 3]))).toEqual([{ inicio: 1.3, fim: 2.4 }])
  })

  it('corridas separadas dão intervalos separados', () => {
    expect(intervalosDasPalavras(ws, new Set([0, 3]))).toEqual([
      { inicio: 0, fim: 0.6 },
      { inicio: 1.6, fim: 2.4 },
    ])
  })

  it('a última palavra corta até ao próprio fim', () => {
    expect(intervalosDasPalavras(ws, new Set([5]))).toEqual([{ inicio: 3.1, fim: 3.5 }])
  })

  it('nada seleccionado, nada cortado', () => {
    expect(intervalosDasPalavras(ws, new Set())).toEqual([])
  })
})

describe('tradução', () => {
  it('parte textos longos em pedaços ≤ 500, preferindo fim de frase', () => {
    const frase = 'Esta é uma frase de teste com algumas palavras. '
    const longo = frase.repeat(30)
    const partes = partirTexto(longo)
    expect(partes.length).toBeGreaterThan(1)
    for (const p of partes) expect(p.length).toBeLessThanOrEqual(500)
    expect(partes[0].endsWith('.')).toBe(true)
    expect(partes.join(' ').replace(/\s+/g, ' ')).toBe(longo.trim().replace(/\s+/g, ' '))
  })

  it('uma palavra gigante é partida à força', () => {
    expect(partirTexto('x'.repeat(1200)).map((p) => p.length)).toEqual([500, 500, 200])
  })

  it('traduz cue a cue, mantém os tempos e nunca envia mais de 500 caracteres', async () => {
    const pedidos: string[] = []
    const traduzir = vi.fn(async (t: string) => {
      pedidos.push(t)
      return `[en] ${t.length}`
    })
    const progresso: number[] = []
    const cues: Cue[] = [
      { id: 'a', inicio: 0, fim: 1, texto: 'Olá' },
      { id: 'b', inicio: 1, fim: 9, texto: 'Longo. '.repeat(100) },
    ]
    const out = await traduzirCues(cues, 'en', traduzir, (f) => progresso.push(f))
    expect(out.map((c) => [c.inicio, c.fim])).toEqual([
      [0, 1],
      [1, 9],
    ])
    expect(out[0].texto).toBe('[en] 3')
    for (const p of pedidos) expect(p.length).toBeLessThanOrEqual(500)
    expect(pedidos.length).toBeGreaterThan(2)
    expect(progresso.sort()).toEqual([1, 2])
  })

  it('pára quando se cancela', async () => {
    const ctl = new AbortController()
    ctl.abort()
    await expect(traduzirCues([{ id: 'a', inicio: 0, fim: 1, texto: 'x' }], 'en', async (t) => t, undefined, ctl.signal)).rejects.toThrow()
  })
})
