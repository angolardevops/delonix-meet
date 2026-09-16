/**
 * A matemática e as preferências do palco do Estúdio — sem canvas, sem DOM,
 * testável em Node.
 *
 * O que vive aqui: os quatro LAYOUTS (e os rectângulos de cada um), a
 * QUALIDADE de gravação (1080p/2160p a 30/50 fps), a etiqueta de PLATAFORMA
 * que se lê do URL RTMP, as SOBREPOSIÇÕES e a MISTURA de áudio com a sua
 * persistência em `localStorage`, e os formatos de número que o ecrã mostra.
 *
 * Os layouts não reinventam a grelha: `lado-a-lado` e `grelha` são o
 * `calcularRects` do compositor da sala. Só o `destaque` (um grande e uma
 * coluna) é novo, porque a sala não o tem.
 */
import { calcularRects, type Rect } from '../room/compositor'

// ------------------------------------------------------------------ layouts

/**
 * - `solo`: o conteúdo (ecrã ou quadro) a ecrã inteiro, com a tua câmara na
 *   bolha por cima — o comportamento de sempre do Estúdio;
 * - `lado-a-lado`: as duas primeiras fontes, metade cada;
 * - `destaque`: a primeira grande, as seguintes numa coluna à direita;
 * - `grelha`: todas, em grelha.
 */
export type LayoutDoPalco = 'solo' | 'lado-a-lado' | 'destaque' | 'grelha'

export const LAYOUTS: LayoutDoPalco[] = ['solo', 'lado-a-lado', 'destaque', 'grelha']

/** Quantas fontes um layout mostra no máximo (a coluna do destaque leva três). */
export const MAXIMO_POR_LAYOUT: Record<LayoutDoPalco, number> = {
  solo: 1,
  'lado-a-lado': 2,
  destaque: 4,
  grelha: 9,
}

/** Fracção da largura que a fonte principal ocupa no `destaque`. */
export const LARGURA_DO_DESTAQUE = 0.7

/**
 * Os rectângulos para `n` fontes num layout. Nunca devolve mais rectângulos
 * do que o layout mostra — quem desenha usa `rects.length`, não `n`.
 */
export function rectsDoLayout(layout: LayoutDoPalco, n: number, largura: number, altura: number): Rect[] {
  const k = Math.min(n, MAXIMO_POR_LAYOUT[layout])
  if (k <= 0) return []
  if (layout === 'destaque' && k > 1) {
    const wPrincipal = Math.round(largura * LARGURA_DO_DESTAQUE)
    const wColuna = largura - wPrincipal
    const hCada = altura / (k - 1)
    return [
      { x: 0, y: 0, w: wPrincipal, h: altura },
      ...Array.from({ length: k - 1 }, (_, i) => ({ x: wPrincipal, y: i * hCada, w: wColuna, h: hCada })),
    ]
  }
  return calcularRects(k, largura, altura)
}

/** O que enche o palco: as fontes, o cartão de intervalo com a marca, ou o quadro. */
export type ConteudoDoPalco = 'fontes' | 'marca' | 'quadro'

/**
 * As tintas do quadro local. São cores de IMAGEM (vão para o vídeo como
 * pixéis), não de interface — por isso não são tokens: o tema escuro não pode
 * mudar a cor de um traço já gravado.
 */
export const TINTAS_DO_QUADRO = ['#111418', '#d92d20', '#1f6feb', '#1a7f37'] as const

// ------------------------------------------------------------------ qualidade

export type Qualidade = '1080p30' | '1080p50' | '2160p30' | '2160p50'

export interface PerfilDeQualidade {
  largura: number
  altura: number
  fps: number
  /** Débito do `MediaRecorder` da gravação local, em bits/s. */
  bitrate: number
}

/**
 * Débitos na ordem do que o YouTube recomenda para cada perfil (SDR). A
 * gravação é LOCAL — não vai pela rede — por isso o tecto é o disco e o
 * encoder, não a banda.
 */
export const QUALIDADES: Record<Qualidade, PerfilDeQualidade> = {
  '1080p30': { largura: 1920, altura: 1080, fps: 30, bitrate: 6_000_000 },
  '1080p50': { largura: 1920, altura: 1080, fps: 50, bitrate: 9_000_000 },
  '2160p30': { largura: 3840, altura: 2160, fps: 30, bitrate: 25_000_000 },
  '2160p50': { largura: 3840, altura: 2160, fps: 50, bitrate: 35_000_000 },
}

export const QUALIDADE_INICIAL: Qualidade = '1080p30'

export function ehQualidade(v: unknown): v is Qualidade {
  return typeof v === 'string' && v in QUALIDADES
}

/** «2160p · 50 fps» — o rótulo técnico, igual em todas as línguas. */
export function rotuloDaQualidade(q: Qualidade): string {
  const p = QUALIDADES[q]
  return `${p.altura}p · ${p.fps} fps`
}

/** 4K é o que o selo REC do topo anuncia; abaixo disso não diz nada a mais. */
export function eh4k(q: Qualidade): boolean {
  return QUALIDADES[q].altura >= 2160
}

// ------------------------------------------------------------------ plataformas

export type Plataforma = 'YT' | 'FB' | 'LI' | 'TW' | 'TT' | 'IG' | 'X' | 'SRV' | 'RTMP'

/**
 * A etiqueta de um destino, lida do HOST do URL RTMP. `SRV` é um servidor
 * nosso (o mesmo host da app, `localhost`, um nome `.local`/`.lan` ou um IP
 * privado); `RTMP` é tudo o resto. Não se adivinha pelo rótulo que a pessoa
 * escreveu — o rótulo é texto livre, o host é o que recebe a imagem.
 */
export function plataformaDoUrl(url: string, hostDaApp = ''): Plataforma {
  const host = hostDoUrl(url)
  if (!host) return 'RTMP'
  const termina = (...sufixos: string[]) => sufixos.some((s) => host === s || host.endsWith(`.${s}`))
  if (termina('youtube.com', 'youtu.be')) return 'YT'
  if (termina('facebook.com', 'fbcdn.net')) return 'FB'
  if (termina('linkedin.com')) return 'LI'
  if (termina('twitch.tv', 'live-video.net')) return 'TW'
  if (termina('tiktok.com', 'tiktokcdn.com')) return 'TT'
  if (termina('instagram.com')) return 'IG'
  if (termina('x.com', 'twitter.com', 'pscp.tv', 'periscope.tv')) return 'X'
  const app = hostDaApp.toLowerCase().replace(/:\d+$/, '')
  if (app && host === app) return 'SRV'
  if (host === 'localhost' || termina('local', 'lan', 'internal', 'localdomain')) return 'SRV'
  if (ipPrivado(host)) return 'SRV'
  return 'RTMP'
}

function hostDoUrl(url: string): string {
  const m = url.trim().match(/^[a-z][a-z0-9+.-]*:\/\/(?:[^@/]*@)?(\[[^\]]+\]|[^:/?#]+)/i)
  return m ? m[1].toLowerCase() : ''
}

function ipPrivado(host: string): boolean {
  const m = host.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/)
  if (!m) return host === '[::1]' || /^\[f[cd][0-9a-f]{2}:/i.test(host)
  const [a, b] = [Number(m[1]), Number(m[2])]
  return (
    a === 10 ||
    a === 127 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 100 && b >= 64 && b <= 127)
  )
}

// ------------------------------------------------------------------ sobreposições

export interface Sobreposicoes {
  rodape: boolean
  logotipo: boolean
  cronometro: boolean
  sondagem: boolean
  legendas: boolean
  ticker: boolean
  /** Rodapé: nome e cargo de quem fala. */
  nome: string
  cargo: string
  /** Texto do ticker (um URL, normalmente). */
  url: string
  /** Minutos de contagem decrescente; 0 = tempo decorrido desde que ligou. */
  minutos: number
}

export const SOBREPOSICOES_INICIAIS: Sobreposicoes = {
  rodape: false,
  logotipo: false,
  cronometro: false,
  sondagem: false,
  legendas: false,
  ticker: false,
  nome: '',
  cargo: '',
  url: '',
  minutos: 0,
}

// ------------------------------------------------------------------ mistura

/** Os três barramentos: a voz (microfone e convidados), a música e o som do ecrã. */
export interface Mistura {
  palco: number
  musica: number
  video: number
}

export const MISTURA_INICIAL: Mistura = { palco: 1, musica: 0.35, video: 1 }

/** Ganho seguro: 0–1,5 (um pouco de reforço, nunca um clip garantido). */
export function ganhoValido(v: unknown, omissao: number): number {
  const n = typeof v === 'number' && Number.isFinite(v) ? v : omissao
  return Math.min(1.5, Math.max(0, n))
}

// ------------------------------------------------------------------ persistência

const CHAVES = {
  qualidade: 'dx_studio_qualidade',
  layout: 'dx_studio_layout',
  sobreposicoes: 'dx_studio_sobreposicoes',
  mistura: 'dx_studio_mistura',
  microfone: 'dx_studio_microfone',
} as const

/** Um `localStorage` que pode não existir (modo privado, Node) nunca parte o palco. */
function lerTexto(chave: string): string | null {
  try {
    return globalThis.localStorage?.getItem(chave) ?? null
  } catch {
    return null
  }
}
function escreverTexto(chave: string, valor: string): void {
  try {
    globalThis.localStorage?.setItem(chave, valor)
  } catch {
    /* sem armazenamento: vale para esta sessão */
  }
}

export function lerQualidade(): Qualidade {
  const v = lerTexto(CHAVES.qualidade)
  return ehQualidade(v) ? v : QUALIDADE_INICIAL
}
export const guardarQualidade = (q: Qualidade) => escreverTexto(CHAVES.qualidade, q)

export function lerLayout(): LayoutDoPalco {
  const v = lerTexto(CHAVES.layout)
  return (LAYOUTS as string[]).includes(v ?? '') ? (v as LayoutDoPalco) : 'solo'
}
export const guardarLayout = (l: LayoutDoPalco) => escreverTexto(CHAVES.layout, l)

/** Lê e SANEIA: um JSON velho ou mexido à mão não pode dar tipos errados ao desenho. */
export function sanearSobreposicoes(v: unknown): Sobreposicoes {
  const o = (v && typeof v === 'object' ? v : {}) as Record<string, unknown>
  const bool = (k: keyof Sobreposicoes) =>
    typeof o[k] === 'boolean' ? (o[k] as boolean) : (SOBREPOSICOES_INICIAIS[k] as boolean)
  const str = (k: keyof Sobreposicoes, max: number) => (typeof o[k] === 'string' ? (o[k] as string).slice(0, max) : '')
  const min = Number(o.minutos)
  return {
    rodape: bool('rodape'),
    logotipo: bool('logotipo'),
    cronometro: bool('cronometro'),
    sondagem: bool('sondagem'),
    legendas: bool('legendas'),
    ticker: bool('ticker'),
    nome: str('nome', 60),
    cargo: str('cargo', 80),
    url: str('url', 80),
    minutos: Number.isFinite(min) ? Math.min(600, Math.max(0, Math.round(min))) : 0,
  }
}

export function lerSobreposicoes(): Sobreposicoes {
  try {
    return sanearSobreposicoes(JSON.parse(lerTexto(CHAVES.sobreposicoes) ?? '{}'))
  } catch {
    return { ...SOBREPOSICOES_INICIAIS }
  }
}
export const guardarSobreposicoes = (s: Sobreposicoes) => escreverTexto(CHAVES.sobreposicoes, JSON.stringify(s))

export function lerMistura(): Mistura {
  try {
    const o = JSON.parse(lerTexto(CHAVES.mistura) ?? '{}') as Record<string, unknown>
    return {
      palco: ganhoValido(o.palco, MISTURA_INICIAL.palco),
      musica: ganhoValido(o.musica, MISTURA_INICIAL.musica),
      video: ganhoValido(o.video, MISTURA_INICIAL.video),
    }
  } catch {
    return { ...MISTURA_INICIAL }
  }
}
export const guardarMistura = (m: Mistura) => escreverTexto(CHAVES.mistura, JSON.stringify(m))

export const lerMicrofone = () => lerTexto(CHAVES.microfone) ?? ''
export const guardarMicrofone = (id: string) => escreverTexto(CHAVES.microfone, id)

// ------------------------------------------------------------------ formatos

/** `h:mm:ss` a partir da primeira hora; `mm:ss` antes. */
export function hhmmss(segundos: number): string {
  const total = Math.max(0, Math.floor(segundos))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  const dd = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${dd(h)}:${dd(m)}:${dd(s)}` : `${dd(m)}:${dd(s)}`
}

/** Bytes legíveis (KB, MB, GB) com a vírgula da língua. */
export function formatarBytes(bytes: number, locale = 'pt-PT'): string {
  const unidades = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = Math.max(0, bytes)
  let i = 0
  while (v >= 1024 && i < unidades.length - 1) {
    v /= 1024
    i++
  }
  const casas = i === 0 ? 0 : v < 10 ? 2 : 1
  return `${v.toLocaleString(locale, { minimumFractionDigits: casas, maximumFractionDigits: casas })} ${unidades[i]}`
}
