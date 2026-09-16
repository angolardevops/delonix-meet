/**
 * Legendas: das palavras com tempo às cues, das cues a SRT/VTT, e do texto
 * seleccionado aos intervalos a cortar.
 *
 * Tudo puro e testado (`legendas.test.ts`). O que corre no browser — o Whisper
 * e a tradução — está em `transcricao.ts` e no painel.
 */
import { juntarPalavras, novoId, normalizarIntervalos } from '../edit/projecto'
import type { Cue, Intervalo, Palavra } from '../edit/projecto'

// ---------------------------------------------------------------------------
//  Tempos
// ---------------------------------------------------------------------------

function doisDigitos(n: number, largura = 2): string {
  return String(n).padStart(largura, '0')
}

/** `00:01:02,345` (SRT) ou `00:01:02.345` (VTT). */
export function tempoDeLegenda(s: number, separador: ',' | '.'): string {
  const ms = Math.max(0, Math.round(s * 1000))
  const h = Math.floor(ms / 3_600_000)
  const m = Math.floor((ms % 3_600_000) / 60_000)
  const seg = Math.floor((ms % 60_000) / 1000)
  return `${doisDigitos(h)}:${doisDigitos(m)}:${doisDigitos(seg)}${separador}${doisDigitos(ms % 1000, 3)}`
}

/** Lê `hh:mm:ss,mmm`, `hh:mm:ss.mmm` ou `mm:ss.mmm`. `NaN` se não for tempo. */
export function lerTempo(txt: string): number {
  const m = txt.trim().match(/^(?:(\d+):)?(\d{1,2}):(\d{1,2})[.,](\d{1,3})$/)
  if (!m) return NaN
  const [, h, mi, s, frac] = m
  return (Number(h ?? 0) * 3600) + Number(mi) * 60 + Number(s) + Number(frac.padEnd(3, '0')) / 1000
}

/** `HH:MM:SS:FF` — o timecode com frame que o template mostra. */
export function timecode(s: number, fps = 30): string {
  const total = Math.max(0, Math.floor(s * fps + 1e-6))
  const f = total % fps
  const seg = Math.floor(total / fps)
  return `${doisDigitos(Math.floor(seg / 3600))}:${doisDigitos(Math.floor(seg / 60) % 60)}:${doisDigitos(seg % 60)}:${doisDigitos(f)}`
}

/**
 * Lê o que alguém escreve num campo de tempo: `hh:mm:ss:ff`, `hh:mm:ss`,
 * `mm:ss`, `ss` — com decimais opcionais nos segundos. `NaN` se não for tempo.
 */
export function lerTimecode(txt: string, fps = 30): number {
  const s = txt.trim().replace(',', '.')
  if (!/^\d+(:\d+){0,3}(\.\d+)?$/.test(s)) return NaN
  const partes = s.split(':')
  let frames = 0
  if (partes.length === 4) frames = Number(partes.pop())
  let total = 0
  for (const p of partes) total = total * 60 + Number(p)
  return total + frames / fps
}

/** `mm:ss` ou `h:mm:ss`. */
export function relogio(s: number): string {
  const t = Math.max(0, Math.floor(s))
  const h = Math.floor(t / 3600)
  const m = Math.floor(t / 60) % 60
  const seg = t % 60
  return h ? `${h}:${doisDigitos(m)}:${doisDigitos(seg)}` : `${doisDigitos(m)}:${doisDigitos(seg)}`
}

// ---------------------------------------------------------------------------
//  SRT / VTT
// ---------------------------------------------------------------------------

function limparTexto(t: string): string {
  // Uma linha em branco dentro de uma cue fecha-a nos dois formatos, e `-->`
  // no texto confunde o leitor de VTT.
  return t.replace(/\r/g, '').replace(/\n{2,}/g, '\n').replace(/-->/g, '→').trim()
}

export function paraSrt(cues: Cue[]): string {
  return (
    cues
      .filter((c) => c.texto.trim() && c.fim > c.inicio)
      .map((c, i) => `${i + 1}\n${tempoDeLegenda(c.inicio, ',')} --> ${tempoDeLegenda(c.fim, ',')}\n${limparTexto(c.texto)}\n`)
      .join('\n')
  )
}

export interface OpcoesVtt {
  /** Etiquetas de tempo por palavra (`<00:00:01.200>`) — o karaoke do VTT. */
  karaoke?: boolean
  /** Orador como `<v Nome>`. */
  oradores?: boolean
}

export function paraVtt(cues: Cue[], o: OpcoesVtt = {}): string {
  const corpo = cues
    .filter((c) => c.texto.trim() && c.fim > c.inicio)
    .map((c, i) => {
      let texto = limparTexto(c.texto)
      if (o.karaoke && c.palavras?.length) {
        texto = c.palavras
          .map((w, j) => (j === 0 ? w.texto.trim() : `<${tempoDeLegenda(w.inicio, '.')}>${w.texto.trim()}`))
          .join(' ')
      }
      if (o.oradores && c.orador) texto = `<v ${c.orador.replace(/[<>]/g, '')}>${texto}`
      return `${i + 1}\n${tempoDeLegenda(c.inicio, '.')} --> ${tempoDeLegenda(c.fim, '.')}\n${texto}\n`
    })
    .join('\n')
  return `WEBVTT\n\n${corpo}`
}

/** Lê SRT ou VTT. Linhas que não são cue são ignoradas, não rebentam. */
export function lerLegendas(texto: string): Cue[] {
  const blocos = texto.replace(/\r/g, '').replace(/^﻿/, '').split(/\n{2,}/)
  const out: Cue[] = []
  for (const b of blocos) {
    const linhas = b.split('\n').filter((l) => l.trim() !== '')
    const i = linhas.findIndex((l) => l.includes('-->'))
    if (i < 0) continue
    const [a, resto] = linhas[i].split('-->')
    const inicio = lerTempo(a)
    const fim = lerTempo((resto ?? '').trim().split(/\s+/)[0] ?? '')
    if (!Number.isFinite(inicio) || !Number.isFinite(fim) || fim <= inicio) continue
    let corpo = linhas.slice(i + 1).join('\n')
    let orador: string | undefined
    const v = corpo.match(/^<v ([^>]+)>/)
    if (v) {
      orador = v[1]
      corpo = corpo.slice(v[0].length)
    }
    corpo = corpo.replace(/<\d{2}:\d{2}:\d{2}\.\d{3}>/g, '').replace(/<\/?[^>]+>/g, '').replace(/[ \t]+/g, ' ').trim()
    out.push({ id: novoId('q'), inicio, fim, texto: corpo, ...(orador ? { orador } : {}) })
  }
  return out
}

// ---------------------------------------------------------------------------
//  Palavras → cues
// ---------------------------------------------------------------------------

export interface RegrasDeCue {
  /** Caracteres por cue (duas linhas de ~42). */
  maxCaracteres: number
  maxDuracao: number
  /** Uma pausa maior do que isto fecha a cue. */
  pausa: number
}
export const REGRAS: RegrasDeCue = { maxCaracteres: 84, maxDuracao: 6, pausa: 0.8 }

/** Agrupa palavras em cues legíveis: fecha em pontuação final, pausa, tamanho ou duração. */
export function palavrasParaCues(palavras: Palavra[], regras: RegrasDeCue = REGRAS, orador?: string): Cue[] {
  const ordenadas = palavras.filter((w) => w.texto.trim()).sort((a, b) => a.inicio - b.inicio)
  const out: Cue[] = []
  let actual: Palavra[] = []
  const fechar = () => {
    if (!actual.length) return
    out.push({
      id: novoId('q'),
      inicio: actual[0].inicio,
      fim: Math.max(actual[actual.length - 1].fim, actual[0].inicio + 0.2),
      texto: juntarPalavras(actual),
      palavras: actual,
      ...(orador ? { orador } : {}),
    })
    actual = []
  }
  for (const w of ordenadas) {
    if (actual.length) {
      const ult = actual[actual.length - 1]
      const texto = juntarPalavras([...actual, w])
      if (
        w.inicio - ult.fim > regras.pausa ||
        texto.length > regras.maxCaracteres ||
        w.fim - actual[0].inicio > regras.maxDuracao
      ) {
        fechar()
      }
    }
    actual.push(w)
    if (/[.!?…]["»)]?$/.test(w.texto.trim()) && juntarPalavras(actual).length > regras.maxCaracteres / 3) fechar()
  }
  fechar()
  return out
}

/**
 * Quando o modelo só dá tempos por SEGMENTO, as palavras recebem tempos
 * proporcionais ao comprimento. É uma estimativa — e o projecto marca-a como
 * tal (`estimadas`), para o corte pelo texto avisar que pode não ser exacto.
 */
export function distribuirPalavras(texto: string, inicio: number, fim: number): Palavra[] {
  const tokens = texto.trim().split(/\s+/).filter(Boolean)
  if (!tokens.length || fim <= inicio) return []
  const pesos = tokens.map((t) => Math.max(1, t.replace(/[^\p{L}\p{N}]/gu, '').length))
  const total = pesos.reduce((a, b) => a + b, 0)
  const out: Palavra[] = []
  let t = inicio
  tokens.forEach((tok, i) => {
    const d = ((fim - inicio) * pesos[i]) / total
    out.push({ inicio: t, fim: t + d, texto: tok })
    t += d
  })
  return out
}

// ---------------------------------------------------------------------------
//  Palavras de preenchimento
// ---------------------------------------------------------------------------

/**
 * Palavras que quase sempre são enchimento. «tipo» e «pronto» também são
 * palavras a sério — por isso nada se apaga sozinho: a interface mostra as
 * ocorrências e é quem edita que decide.
 */
export const PREENCHIMENTO: Record<string, readonly string[]> = {
  pt: ['pá', 'tipo', 'né', 'hum', 'hmm', 'ahm', 'eh', 'ah', 'éh', 'uh', 'pronto', 'portanto', 'quer dizer', 'ou seja'],
  en: ['um', 'uh', 'er', 'ah', 'like', 'you know', 'i mean', 'basically', 'actually'],
  fr: ['euh', 'bah', 'ben', 'genre', 'du coup', 'voilà', 'hein'],
  es: ['eh', 'este', 'o sea', 'pues', 'bueno', 'tipo'],
  de: ['äh', 'ähm', 'hm', 'halt', 'also', 'sozusagen'],
}

export function normalizarPalavra(t: string): string {
  return t
    .toLowerCase()
    .normalize('NFC')
    .replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '')
}

export interface Ocorrencia {
  termo: string
  /** Índices em `palavras` (uma expressão pode ocupar várias). */
  indices: number[]
}

/**
 * Encontra enchimentos (incluindo expressões de várias palavras). `extra` são
 * termos vindos de fora da lista fixa — os que o LLM local encontrou NESTA
 * transcrição (o servidor só devolve termos que lá estão).
 */
export function encontrarPreenchimento(palavras: Palavra[], lingua: string, extra: readonly string[] = []): Ocorrencia[] {
  const fixos = PREENCHIMENTO[lingua.split('-')[0]] ?? []
  const vistos = new Set(fixos)
  const novos = extra.map((x) => x.split(/\s+/).map(normalizarPalavra).filter(Boolean).join(' ')).filter((x) => x && !vistos.has(x) && (vistos.add(x), true))
  // Expressões mais longas primeiro: «quer dizer» não pode ser apanhada como «quer».
  const termos = [...fixos, ...novos].sort((a, b) => b.split(' ').length - a.split(' ').length)
  const norm = palavras.map((w) => normalizarPalavra(w.texto))
  const out: Ocorrencia[] = []
  for (let i = 0; i < norm.length; i++) {
    for (const termo of termos) {
      const partes = termo.split(' ')
      if (partes.every((p, k) => norm[i + k] === p)) {
        out.push({ termo, indices: partes.map((_, k) => i + k) })
        i += partes.length - 1
        break
      }
    }
  }
  return out
}

/** Contagem por termo, para o resumo «18 pá, 9 tipo». */
export function contarPreenchimento(ocorrencias: Ocorrencia[]): { termo: string; n: number }[] {
  const m = new Map<string, number>()
  for (const o of ocorrencias) m.set(o.termo, (m.get(o.termo) ?? 0) + 1)
  return [...m.entries()].map(([termo, n]) => ({ termo, n })).sort((a, b) => b.n - a.n)
}

// ---------------------------------------------------------------------------
//  Corte pelo texto
// ---------------------------------------------------------------------------

/**
 * Intervalos a remover quando se apagam as palavras seleccionadas.
 *
 * Cada corrida de palavras seguidas sai de uma vez, do início da primeira ao
 * INÍCIO da palavra que fica a seguir — assim o silêncio entre a última
 * palavra apagada e a seguinte também sai e a frase fecha sem buraco. A última
 * palavra do texto corta até ao seu próprio fim.
 */
export function intervalosDasPalavras(palavras: Palavra[], seleccionadas: Set<number>): Intervalo[] {
  const ord = palavras.map((w, i) => ({ w, i })).sort((a, b) => a.w.inicio - b.w.inicio)
  const out: Intervalo[] = []
  let k = 0
  while (k < ord.length) {
    if (!seleccionadas.has(ord[k].i)) {
      k++
      continue
    }
    const inicio = ord[k].w.inicio
    let fim = ord[k].w.fim
    while (k < ord.length && seleccionadas.has(ord[k].i)) {
      fim = Math.max(fim, ord[k].w.fim)
      k++
    }
    if (k < ord.length) fim = Math.max(fim, ord[k].w.inicio)
    out.push({ inicio, fim })
  }
  return normalizarIntervalos(out)
}

/** Todas as palavras das cues, por ordem, com o índice da cue de onde vêm. */
export function palavrasDasCues(cues: Cue[]): { palavra: Palavra; cue: number }[] {
  const out: { palavra: Palavra; cue: number }[] = []
  cues.forEach((c, i) => {
    const ws = c.palavras?.length ? c.palavras : distribuirPalavras(c.texto, c.inicio, c.fim)
    for (const w of ws) out.push({ palavra: w, cue: i })
  })
  return out
}

/**
 * Substitui o texto de uma cue editada à mão. Com o mesmo número de palavras
 * os tempos medidos ficam; com outro número são redistribuídos (e a cue
 * deixa de ter tempos medidos — é a verdade).
 */
export function editarTextoDaCue(c: Cue, texto: string): Cue {
  const tokens = texto.trim().split(/\s+/).filter(Boolean)
  if (c.palavras && tokens.length === c.palavras.length) {
    const palavras = c.palavras.map((w, i) => ({ ...w, texto: tokens[i] }))
    return { ...c, texto: juntarPalavras(palavras), palavras }
  }
  return { ...c, texto: texto.trim(), palavras: distribuirPalavras(texto, c.inicio, c.fim) }
}

// ---------------------------------------------------------------------------
//  Tradução
// ---------------------------------------------------------------------------

/** O servidor recusa textos acima disto (`ai.rs`, `POST /api/translate`). */
export const LIMITE_TRADUCAO = 500

/**
 * Parte um texto em pedaços ≤ `limite`, primeiro em fim de frase, depois em
 * espaço, e só em último caso a meio de uma palavra.
 */
export function partirTexto(texto: string, limite = LIMITE_TRADUCAO): string[] {
  const t = texto.trim()
  if (t.length <= limite) return t ? [t] : []
  const out: string[] = []
  let resto = t
  while (resto.length > limite) {
    const janela = resto.slice(0, limite + 1)
    let corte = Math.max(janela.lastIndexOf('. '), janela.lastIndexOf('? '), janela.lastIndexOf('! '))
    if (corte > limite / 3) corte += 1
    else corte = janela.lastIndexOf(' ')
    if (corte <= 0) corte = limite
    out.push(resto.slice(0, corte).trim())
    resto = resto.slice(corte).trim()
  }
  if (resto) out.push(resto)
  return out
}

/** Línguas que o servidor traduz hoje (`ai.rs`). Umbundu, Kimbundu, Kikongo e Mandarim não. */
export const LINGUAS_DE_TRADUCAO = ['en', 'fr', 'es', 'de', 'pt'] as const

/**
 * Traduz cue a cue — um pedido por cue, porque juntar várias num pedido e
 * separar depois pela quebra de linha depende de o modelo a preservar, e
 * quando não preserva as legendas saem desalinhadas sem aviso.
 */
export async function traduzirCues(
  cues: Cue[],
  alvo: string,
  traduzir: (texto: string, alvo: string) => Promise<string>,
  aoProgredir?: (feitas: number, total: number) => void,
  sinal?: AbortSignal,
  paralelo = 3,
): Promise<Cue[]> {
  const out: Cue[] = new Array(cues.length)
  let proxima = 0
  let feitas = 0
  const trabalhador = async () => {
    while (proxima < cues.length) {
      if (sinal?.aborted) throw new DOMException('abortado', 'AbortError')
      const i = proxima++
      const c = cues[i]
      const partes = partirTexto(c.texto)
      const traduzidas: string[] = []
      for (const p of partes) traduzidas.push((await traduzir(p, alvo)).trim())
      out[i] = { id: novoId('q'), inicio: c.inicio, fim: c.fim, texto: traduzidas.join(' '), ...(c.orador ? { orador: c.orador } : {}) }
      feitas++
      aoProgredir?.(feitas, cues.length)
    }
  }
  await Promise.all(Array.from({ length: Math.min(paralelo, Math.max(1, cues.length)) }, trabalhador))
  return out
}
