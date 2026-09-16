/**
 * Predefinições de exportação que o BROWSER consegue cumprir.
 *
 * O template mostra também «Aula 4K · H.265 · 24 Mbps». Não está aqui: o
 * WebCodecs do Chrome não codifica H.265 na maioria dos dispositivos, e um 4K
 * em software no browser leva horas. Isso é trabalho da fila de exportação no
 * servidor, que ainda não existe — e uma predefinição que não se cumpre é pior
 * do que nenhuma.
 *
 * Idem MP3: não há codificador de MP3 nas dependências. O podcast sai em Opus
 * (WebM de áudio), que é o que o browser codifica nativamente, e o rótulo diz
 * isso em vez de prometer MP3.
 */

export type IdDaPredefinicao = 'web1080' | 'web720' | 'vertical' | 'podcast'

export interface Predefinicao {
  id: IdDaPredefinicao
  largura: number
  altura: number
  fps: number
  /** bits/s de vídeo; 0 = só áudio. */
  videoBps: number
  audioBps: number
  /** Como a imagem 16:9 entra num quadro de outra proporção. */
  enquadramento: 'caber' | 'preencher'
  soAudio: boolean
  extensao: 'webm' | 'weba'
}

export const PREDEFINICOES: readonly Predefinicao[] = [
  { id: 'web1080', largura: 1920, altura: 1080, fps: 30, videoBps: 8_000_000, audioBps: 128_000, enquadramento: 'caber', soAudio: false, extensao: 'webm' },
  { id: 'web720', largura: 1280, altura: 720, fps: 30, videoBps: 4_000_000, audioBps: 128_000, enquadramento: 'caber', soAudio: false, extensao: 'webm' },
  { id: 'vertical', largura: 1080, altura: 1920, fps: 30, videoBps: 6_000_000, audioBps: 128_000, enquadramento: 'preencher', soAudio: false, extensao: 'webm' },
  { id: 'podcast', largura: 0, altura: 0, fps: 0, videoBps: 0, audioBps: 128_000, enquadramento: 'caber', soAudio: true, extensao: 'weba' },
]

export function predefinicao(id: IdDaPredefinicao): Predefinicao {
  return PREDEFINICOES.find((p) => p.id === id) ?? PREDEFINICOES[0]
}

/** Sobrecarga do contentor WebM (cabeçalhos de cluster e blocos), medida ~2%. */
const SOBRECARGA = 1.02

/** Tamanho estimado em bytes: débito × duração. */
export function tamanhoEstimado(p: Predefinicao, duracao: number): number {
  const bps = p.videoBps + p.audioBps
  return Math.round(((bps * Math.max(0, duracao)) / 8) * SOBRECARGA)
}

/**
 * Tempo estimado de exportação, em segundos. `fpsMedidos` é o ritmo de
 * codificação medido NESTA máquina na última exportação; sem medição usa-se
 * um valor conservador (software, 1080p).
 */
export function tempoEstimado(p: Predefinicao, duracao: number, fpsMedidos: number | null): number {
  if (p.soAudio) return Math.max(1, duracao / 40)
  const escala = (p.largura * p.altura) / (1920 * 1080)
  const ritmo = fpsMedidos && fpsMedidos > 0 ? fpsMedidos : 20 / Math.max(0.25, escala)
  return Math.max(1, (duracao * p.fps) / ritmo)
}

/** `1,2 GB`, `48 MB`, `512 KB` — base 1000, como os sistemas mostram ficheiros. */
export function tamanhoLegivel(bytes: number, locale = 'pt'): string {
  const f = (v: number, casas: number) => v.toLocaleString(locale, { maximumFractionDigits: casas, minimumFractionDigits: 0 })
  if (bytes >= 1e9) return `${f(bytes / 1e9, 1)} GB`
  if (bytes >= 1e6) return `${f(bytes / 1e6, 1)} MB`
  return `${f(Math.max(1, bytes / 1e3), 0)} KB`
}

/** Nome de ficheiro seguro a partir de um título. */
export function nomeDeFicheiro(titulo: string, sufixo: string, extensao: string): string {
  const base = titulo.normalize('NFKD').replace(/[̀-ͯ]/g, '').replace(/[^\w.-]+/g, '-').replace(/^-+|-+$/g, '') || 'projecto'
  return `${base}${sufixo ? `-${sufixo}` : ''}.${extensao}`
}
