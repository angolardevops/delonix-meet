/**
 * Desenho de UM quadro do projecto num canvas — o que a exportação codifica.
 *
 * Ordem: fundo preto → V1 → V2 (cada um com cor, máscara e transição) → textos
 * → legendas queimadas → marca de água.
 */
import { camadaDeTemperatura, filtroCss, posicaoNoCanto } from './cor'
import type { FornecedorDeFrames } from './frames'
import type { Clip, Cue, EstiloDeLegenda, Projecto } from './projecto'
import { clipEm, clipsDaFaixa, cueEm, estadoDaFaixa, fimDoClip, tempoNaFonte } from './projecto'

type Ctx = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D

export interface OpcoesDeDesenho {
  largura: number
  altura: number
  fps: number
  enquadramento: 'caber' | 'preencher'
  /** Língua das legendas a queimar, ou `null`. */
  legendas: string | null
  marcaDeAgua: string | null
  /** Cores lidas dos tokens na interface (o canvas não lê CSS). */
  destaque: string
  familia: string
}

export class Desenhador {
  private fornecedores = new Map<string, Promise<FornecedorDeFrames>>()
  /** Último frame do clipe anterior, para dissolver sem obrigar o decoder a recuar. */
  private ultimos = new Map<string, ImageBitmap | null>()

  constructor(
    private readonly p: Projecto,
    private readonly abrir: (fonteId: string) => Promise<FornecedorDeFrames>,
    private readonly o: OpcoesDeDesenho,
  ) {}

  private fornecedor(id: string): Promise<FornecedorDeFrames> {
    let f = this.fornecedores.get(id)
    if (!f) {
      f = this.abrir(id)
      this.fornecedores.set(id, f)
    }
    return f
  }

  async quadro(ctx: Ctx, t: number): Promise<void> {
    const { largura: W, altura: H } = this.o
    ctx.save()
    ctx.globalAlpha = 1
    ctx.filter = 'none'
    ctx.fillStyle = '#000'
    ctx.fillRect(0, 0, W, H)
    ctx.restore()

    for (const faixa of ['V1', 'V2'] as const) {
      if (!estadoDaFaixa(this.p, faixa).visivel) continue
      const c = clipEm(this.p, faixa, t)
      if (!c) continue
      let alfa = 1
      if (c.transicao && t - c.inicio < c.transicao.duracao) {
        const prog = Math.max(0, Math.min(1, (t - c.inicio) / c.transicao.duracao))
        if (c.transicao.tipo === 'dissolver') {
          const antes = this.anterior(c)
          if (antes) {
            const img = await this.ultimoFrame(antes)
            if (img) this.desenharFonte(ctx, img, antes, 1)
          }
        }
        alfa = prog
      }
      const f = await this.fornecedor(c.fonteId)
      const img = await f.frameEm(tempoNaFonte(c, t))
      if (img) this.desenharFonte(ctx, img, c, alfa)
    }

    for (const tx of this.p.textos) {
      if (t < tx.inicio || t >= tx.inicio + tx.duracao) continue
      const tam = Math.max(8, tx.tamanho * H)
      ctx.save()
      ctx.font = `700 ${tam}px ${this.o.familia}`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'
      ctx.lineJoin = 'round'
      ctx.lineWidth = tam * 0.14
      ctx.strokeStyle = 'rgba(0,0,0,0.85)'
      ctx.fillStyle = '#fff'
      const linhas = quebrar(ctx, tx.texto, W * 0.9)
      linhas.forEach((l, i) => {
        const y = tx.y * H + (i - (linhas.length - 1) / 2) * tam * 1.15
        ctx.strokeText(l, tx.x * W, y)
        ctx.fillText(l, tx.x * W, y)
      })
      ctx.restore()
    }

    if (this.o.legendas && estadoDaFaixa(this.p, 'CC').visivel) {
      const cue = cueEm(this.p, t, this.o.legendas)
      const segunda = this.p.estilo.segundaLingua && this.p.estilo.segundaLingua !== this.o.legendas ? cueEm(this.p, t, this.p.estilo.segundaLingua) : null
      if (cue || segunda) desenharLegenda(ctx, cue, segunda, t, this.p.estilo, this.o)
    }

    if (this.o.marcaDeAgua && this.p.marca.marcaDeAgua) {
      const tam = Math.round(H * 0.03)
      ctx.save()
      ctx.font = `700 ${tam}px ${this.o.familia}`
      const w = ctx.measureText(this.o.marcaDeAgua).width
      const pos = posicaoNoCanto(this.p.marca.canto, w / W, tam / H)
      ctx.globalAlpha = this.p.marca.opacidade
      ctx.textBaseline = 'top'
      ctx.fillStyle = '#fff'
      ctx.shadowColor = 'rgba(0,0,0,0.6)'
      ctx.shadowBlur = tam * 0.3
      ctx.fillText(this.o.marcaDeAgua, pos.x * W, pos.y * H)
      ctx.restore()
    }
  }

  private anterior(c: Clip): Clip | null {
    const lista = clipsDaFaixa(this.p, c.faixa)
    const antes = lista.filter((o) => o.id !== c.id && Math.abs(fimDoClip(o) - c.inicio) < 1 / this.o.fps + 1e-3)
    return antes[antes.length - 1] ?? null
  }

  private async ultimoFrame(c: Clip): Promise<ImageBitmap | null> {
    if (this.ultimos.has(c.id)) return this.ultimos.get(c.id) ?? null
    const f = await this.fornecedor(c.fonteId)
    const img = await f.frameEm(tempoNaFonte(c, fimDoClip(c) - 1 / this.o.fps))
    const bmp = img ? await createImageBitmap(img) : null
    this.ultimos.set(c.id, bmp)
    return bmp
  }

  private desenharFonte(ctx: Ctx, img: CanvasImageSource, c: Clip, alfa: number): void {
    const { largura: W, altura: H } = this.o
    const sw = dimensao(img, 'w')
    const sh = dimensao(img, 'h')
    if (!sw || !sh) return
    const escala = this.o.enquadramento === 'preencher' ? Math.max(W / sw, H / sh) : Math.min(W / sw, H / sh)
    const dw = sw * escala
    const dh = sh * escala
    const dx = (W - dw) / 2
    const dy = (H - dh) / 2
    const m = c.mascara ?? { x: 0, y: 0, w: 1, h: 1 }
    ctx.save()
    ctx.globalAlpha = alfa
    ctx.filter = filtroCss(c.cor)
    ctx.drawImage(img, m.x * sw, m.y * sh, m.w * sw, m.h * sh, dx + m.x * dw, dy + m.y * dh, m.w * dw, m.h * dh)
    ctx.filter = 'none'
    const temp = camadaDeTemperatura(c.cor)
    if (temp) {
      ctx.globalCompositeOperation = 'soft-light'
      ctx.fillStyle = `rgba(${temp.rgb}, ${temp.alfa * alfa})`
      ctx.globalAlpha = 1
      ctx.fillRect(dx + m.x * dw, dy + m.y * dh, m.w * dw, m.h * dh)
    }
    ctx.restore()
  }

  fechar(): void {
    for (const f of this.fornecedores.values()) void f.then((x) => x.fechar()).catch(() => undefined)
    this.fornecedores.clear()
    for (const b of this.ultimos.values()) b?.close()
    this.ultimos.clear()
  }
}

function dimensao(img: CanvasImageSource, eixo: 'w' | 'h'): number {
  if (typeof VideoFrame !== 'undefined' && img instanceof VideoFrame) return eixo === 'w' ? img.displayWidth : img.displayHeight
  if (typeof HTMLVideoElement !== 'undefined' && img instanceof HTMLVideoElement) return eixo === 'w' ? img.videoWidth : img.videoHeight
  const x = img as { width: number; height: number }
  return eixo === 'w' ? x.width : x.height
}

export function quebrar(ctx: Ctx, texto: string, max: number): string[] {
  const out: string[] = []
  for (const paragrafo of texto.split('\n')) {
    let linha = ''
    for (const palavra of paragrafo.split(/\s+/).filter(Boolean)) {
      const tentativa = linha ? `${linha} ${palavra}` : palavra
      if (linha && ctx.measureText(tentativa).width > max) {
        out.push(linha)
        linha = palavra
      } else linha = tentativa
    }
    if (linha) out.push(linha)
  }
  return out
}

function desenharLegenda(ctx: Ctx, cue: Cue | null, segunda: Cue | null, t: number, e: EstiloDeLegenda, o: OpcoesDeDesenho): void {
  const W = o.largura
  const H = o.altura
  const tam = Math.round((e.tamanho * H) / 1080)
  const tam2 = Math.round(tam * 0.8)
  const maxW = W * 0.84
  ctx.save()
  ctx.textAlign = 'center'
  ctx.textBaseline = 'alphabetic'
  ctx.font = `600 ${tam}px ${o.familia}`
  const linhas = cue ? quebrar(ctx, cue.texto, maxW) : []
  ctx.font = `500 ${tam2}px ${o.familia}`
  const linhas2 = segunda ? quebrar(ctx, segunda.texto, maxW) : []
  const alturaTotal = linhas.length * tam * 1.25 + linhas2.length * tam2 * 1.25
  const base = H - H * 0.07
  let y = base - alturaTotal + tam
  const pad = tam * 0.3

  if (e.modo === 'faixa') {
    ctx.fillStyle = 'rgba(0,0,0,0.62)'
    ctx.fillRect(0, base - alturaTotal - pad, W, alturaTotal + pad * 2)
  }

  const escrever = (l: string, tamanho: number, peso: number, cor: string) => {
    ctx.font = `${peso} ${tamanho}px ${o.familia}`
    const w = ctx.measureText(l).width
    if (e.modo === 'caixa' || e.modo === 'karaoke') {
      ctx.fillStyle = 'rgba(0,0,0,0.78)'
      ctx.fillRect(W / 2 - w / 2 - pad, y - tamanho - pad * 0.2, w + pad * 2, tamanho * 1.25)
    }
    if (e.modo === 'contorno') {
      ctx.lineJoin = 'round'
      ctx.lineWidth = tamanho * 0.16
      ctx.strokeStyle = '#000'
      ctx.strokeText(l, W / 2, y)
    }
    ctx.fillStyle = cor
    ctx.fillText(l, W / 2, y)
  }

  if (cue && e.modo === 'karaoke' && cue.palavras?.length) {
    // Karaoke: as palavras já ditas na cor de destaque. Desenha-se linha a
    // linha com as mesmas quebras, palavra a palavra.
    ctx.font = `600 ${tam}px ${o.familia}`
    let k = 0
    for (const l of linhas) {
      const n = l.split(/\s+/).filter(Boolean).length
      const ws = cue.palavras.slice(k, k + n)
      k += n
      const total = ctx.measureText(l).width
      ctx.fillStyle = 'rgba(0,0,0,0.78)'
      ctx.fillRect(W / 2 - total / 2 - pad, y - tam - pad * 0.2, total + pad * 2, tam * 1.25)
      let x = W / 2 - total / 2
      ctx.textAlign = 'left'
      for (const w of ws) {
        const txt = `${w.texto.trim()} `
        ctx.fillStyle = t >= w.inicio ? o.destaque : '#fff'
        ctx.fillText(txt, x, y)
        x += ctx.measureText(txt).width
      }
      ctx.textAlign = 'center'
      y += tam * 1.25
    }
  } else {
    for (const l of linhas) {
      escrever(l, tam, 600, '#fff')
      y += tam * 1.25
    }
  }
  for (const l of linhas2) {
    escrever(l, tam2, 500, 'rgba(255,255,255,0.86)')
    y += tam2 * 1.25
  }
  ctx.restore()
}
