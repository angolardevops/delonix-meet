/**
 * Do desenho no ecrã para ficheiros: SVG autónomo (o conteúdo do `<svg>` sem
 * os elementos de interface) e PNG rasterizado a partir desse SVG.
 *
 * O PNG é o que vai para a biblioteca (POST /api/whiteboards, limite de
 * 8 MB no servidor). A escala baixa até caber — nunca se manda um pedido que
 * o servidor vai recusar.
 */
import { exportBox, wrapSvg } from './exporters'
import type { DiagramDoc } from './model'

/** SVG do conteúdo desenhado: clona o grupo `[data-content]` e tira o que é interface. */
export function svgFromCanvas(root: SVGSVGElement, doc: DiagramDoc): string {
  const content = root.querySelector('[data-content]')
  if (!content) return wrapSvg('', exportBox(doc), doc.title)
  const clone = content.cloneNode(true) as Element
  clone.querySelectorAll('[data-ui]').forEach((el) => el.remove())
  clone.removeAttribute('transform')
  clone.querySelectorAll('[class]').forEach((el) => el.removeAttribute('class'))
  clone.querySelectorAll('[tabindex],[role],[aria-label],[data-node-id],[data-edge-id],[data-stroke-id]').forEach((el) => {
    for (const a of ['tabindex', 'role', 'aria-label', 'data-node-id', 'data-edge-id', 'data-stroke-id']) el.removeAttribute(a)
  })
  const inner = new XMLSerializer().serializeToString(clone)
  return wrapSvg(inner, exportBox(doc), doc.title)
}

const MAX_PNG = 8 * 1024 * 1024
const MAX_SIDE = 4096

/** PNG em base64 (sem prefixo) a partir de um SVG autónomo. */
export async function pngFromSvg(svg: string): Promise<{ base64: string; blob: Blob }> {
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }))
  try {
    const img = new Image()
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('svg'))
      img.src = url
    })
    const w = img.naturalWidth || 800
    const h = img.naturalHeight || 600
    for (let scale = Math.min(2, MAX_SIDE / Math.max(w, h)); scale > 0.1; scale /= 1.5) {
      const canvas = document.createElement('canvas')
      canvas.width = Math.max(1, Math.round(w * scale))
      canvas.height = Math.max(1, Math.round(h * scale))
      const ctx = canvas.getContext('2d')
      if (!ctx) throw new Error('canvas')
      ctx.drawImage(img, 0, 0, canvas.width, canvas.height)
      const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, 'image/png'))
      if (!blob) throw new Error('png')
      if (blob.size <= MAX_PNG * 0.95) {
        const buf = new Uint8Array(await blob.arrayBuffer())
        let bin = ''
        for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000))
        return { base64: btoa(bin), blob }
      }
    }
    throw new Error('png-grande')
  } finally {
    URL.revokeObjectURL(url)
  }
}

/**
 * Descarrega um ficheiro gerado no browser. É a pessoa que carrega no botão:
 * não há descarga sem gesto.
 */
export function downloadText(name: string, text: string, type: string): void {
  downloadBlob(name, new Blob([text], { type }))
}

export function downloadBlob(name: string, blob: Blob): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
