/**
 * O desenho de um PLANO da mesa num contexto 2D, e das quatro transições.
 *
 * Existe à parte do compositor porque DOIS sítios desenham planos: o
 * compositor (o programa, que vai para o ar e para a gravação) e o monitor de
 * pré-visualização. Duplicar o desenho era ver na pré uma coisa e pôr no ar
 * outra à primeira correcção feita só num lado.
 */
import { type MarcaDoPalco, desenharSimbolo } from '../desenho'
import { rectsDoLayout } from '../palco'
import { bandaDoStinger, type Plano, type QuadroDaMesa, type Rect, rectDaJanela, stingerJaTrocou } from './mesa'

export type ImagemDeFonte = HTMLVideoElement | HTMLCanvasElement

export interface FontesParaDesenho {
  /** A imagem (já corrigida, se houver correcção) de uma fonte, ou `null`. */
  imagem(id: string): ImagemDeFonte | null
  /** `contain` para ecrãs e quadros (não se corta um diapositivo); `cover` para pessoas. */
  ajuste(id: string): 'cover' | 'contain'
}

const dims = (f: ImagemDeFonte) =>
  f instanceof HTMLVideoElement ? { w: f.videoWidth, h: f.videoHeight } : { w: f.width, h: f.height }

export function desenharImagem(c: CanvasRenderingContext2D, f: ImagemDeFonte | null, r: Rect, modo: 'cover' | 'contain'): void {
  c.fillStyle = '#0d0d0f'
  c.fillRect(r.x, r.y, r.w, r.h)
  if (!f) return
  if (f instanceof HTMLVideoElement && f.readyState < 2) return
  const { w, h } = dims(f)
  if (!w || !h) return
  const esc = modo === 'contain' ? Math.min(r.w / w, r.h / h) : Math.max(r.w / w, r.h / h)
  const dw = w * esc
  const dh = h * esc
  c.save()
  c.beginPath()
  c.rect(r.x, r.y, r.w, r.h)
  c.clip()
  c.drawImage(f, r.x + (r.w - dw) / 2, r.y + (r.h - dh) / 2, dw, dh)
  c.restore()
}

export function desenharPlano(c: CanvasRenderingContext2D, plano: Plano | null, fontes: FontesParaDesenho, W: number, H: number): void {
  c.fillStyle = '#000000'
  c.fillRect(0, 0, W, H)
  if (!plano || !plano.fontes.length) return
  if (plano.layout === 'janela' && plano.fontes.length > 1) {
    const [a, b] = plano.fontes
    desenharImagem(c, fontes.imagem(a), { x: 0, y: 0, w: W, h: H }, fontes.ajuste(a))
    const j = rectDaJanela(W, H)
    desenharImagem(c, fontes.imagem(b), j, 'cover')
    c.strokeStyle = 'rgba(255,255,255,0.7)'
    c.lineWidth = Math.max(2, H * 0.003)
    c.strokeRect(j.x, j.y, j.w, j.h)
    return
  }
  const layout = plano.layout === 'janela' ? 'solo' : plano.layout
  const rects = rectsDoLayout(layout, plano.fontes.length, W, H)
  const pad = rects.length > 1 ? Math.round(H * 0.004) : 0
  rects.forEach((r, i) => {
    const id = plano.fontes[i]
    desenharImagem(c, fontes.imagem(id), { x: r.x + pad, y: r.y + pad, w: r.w - pad * 2, h: r.h - pad * 2 }, fontes.ajuste(id))
  })
}

/**
 * Um frame da mesa: o programa, ou a transição a decorrer. `camada` é um
 * canvas de trabalho do mesmo tamanho (o destino desenha-se nele antes de ser
 * fundido ou recortado por cima da origem).
 */
export function desenharQuadroDaMesa(
  c: CanvasRenderingContext2D,
  camada: CanvasRenderingContext2D,
  q: QuadroDaMesa,
  fontes: FontesParaDesenho,
  marca: MarcaDoPalco,
  W: number,
  H: number,
): void {
  const t = q.transicao
  if (!t) {
    desenharPlano(c, q.programa, fontes, W, H)
    return
  }
  if (t.tipo === 'stinger') {
    desenharPlano(c, stingerJaTrocou(t.p) ? t.para : t.de, fontes, W, H)
    const b = bandaDoStinger(t.p, W)
    const g = c.createLinearGradient(b.x, 0, b.x + b.w, 0)
    g.addColorStop(0, 'rgba(173,16,23,0)')
    g.addColorStop(0.18, 'rgba(173,16,23,0.96)')
    g.addColorStop(0.82, 'rgba(173,16,23,0.96)')
    g.addColorStop(1, 'rgba(173,16,23,0)')
    c.fillStyle = g
    c.fillRect(b.x, 0, b.w, H)
    const lado = H * 0.22
    desenharSimbolo(c, marca, b.x + b.w / 2 - lado / 2, H / 2 - lado / 2, lado, '#ffffff')
    return
  }
  desenharPlano(c, t.de, fontes, W, H)
  if (camada.canvas.width !== W || camada.canvas.height !== H) {
    camada.canvas.width = W
    camada.canvas.height = H
  }
  desenharPlano(camada, t.para, fontes, W, H)
  if (t.tipo === 'misturar') {
    c.save()
    c.globalAlpha = t.p
    c.drawImage(camada.canvas, 0, 0)
    c.restore()
    return
  }
  // LIMPAR: a cortina da esquerda para a direita, com um fio de luz na borda.
  const x = Math.round(W * t.p)
  if (x > 0) c.drawImage(camada.canvas, 0, 0, x, H, 0, 0, x, H)
  if (x > 0 && x < W) {
    c.fillStyle = 'rgba(255,255,255,0.85)'
    c.fillRect(x - Math.max(1, W * 0.002), 0, Math.max(2, W * 0.004), H)
  }
}
