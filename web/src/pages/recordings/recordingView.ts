/**
 * CAMADA DE MAPEAMENTO — o ÚNICO sítio da UI de gravações que lê campos da API.
 *
 * Os componentes (biblioteca, painel, leitor) só conhecem `RecordingView`.
 * Hoje a fonte é a `RecordingItem` de sempre (`GET /api/recordings`): nome,
 * sala, autor, data, tamanho, partilha, descarga e falha. Tudo o resto que o
 * template mostra — duração e resolução medidas pelo servidor, categoria da
 * sessão, progresso de processamento/transcrição, organização, visualizações,
 * participantes, comentários, capítulos, descrição e etiquetas, publicação —
 * fica `null` até o contrato de metadados chegar à `main` (linha ADR-0004,
 * `g4-recordings`: `duration_secs`, `title`/`category`, `/metadata`,
 * capítulos com `at_secs`, comentários com `author_*`, listas
 * `{items, next_page_token}`).
 *
 * `null` quer dizer «o servidor não diz», e o ecrã desenha a estrutura sem
 * valor — nunca um número inventado. Ligar o contrato novo é mudar
 * `fromRecordingItem` (e acrescentar os `from*` dos sub-recursos) aqui, sem
 * tocar nos componentes.
 */
import type { RecordingItem } from '../../api'

export type SessionCategory = 'training' | 'hybrid' | 'broadcast' | 'meeting'

/** Fase do ficheiro no servidor. Hoje só há `ready` e `failed`. */
export type Pipeline = 'processing' | 'transcribing' | 'ready' | 'published' | 'failed'

export interface RecordingView {
  /** Item original — só para o passar às funções da API (descarregar, partilhar). */
  source: RecordingItem
  id: string
  name: string
  roomCode: string
  uploaderName: string
  createdAt: string
  /** `null` numa falhada: não há ficheiro, e «0 MB» leria-se como ficheiro vazio. */
  sizeBytes: number | null
  owned: boolean
  shareCount: number
  canDownload: boolean
  failed: boolean
  failureReason: string | null
  pipeline: Pipeline
  // ---- à espera do contrato de metadados (null = o servidor não diz) ----
  durationMs: number | null
  width: number | null
  height: number | null
  category: SessionCategory | null
  progressPct: number | null
  transcriptRunning: boolean
  transcriptLanguage: string | null
  orgName: string | null
  viewCount: number | null
  participantCount: number | null
  commentCount: number | null
  chapterCount: number | null
  description: string | null
  tags: string[] | null
}

/** Capítulo como o ecrã o usa (instante em ms desde o início do vídeo). */
export interface ChapterView {
  id: string
  tMs: number
  title: string
  auto: boolean
}

/** Segmento de transcrição com instante RELATIVO ao vídeo. */
export interface SegmentView {
  startMs: number
  endMs: number
  text: string
}

/** Nome visível: o ficheiro sem a extensão do contentor. */
export function displayName(filename: string): string {
  return filename.replace(/\.(webm|mp4|mkv)$/i, '')
}

export function fromRecordingItem(r: RecordingItem): RecordingView {
  const failed = r.status === 'failed'
  return {
    source: r,
    id: r.id,
    name: displayName(r.filename),
    roomCode: r.room_code,
    uploaderName: r.uploader_name,
    createdAt: r.created_at,
    sizeBytes: failed ? null : r.size_bytes,
    owned: r.owned,
    shareCount: r.share_count,
    canDownload: r.can_download,
    failed,
    failureReason: r.failure_reason,
    pipeline: failed ? 'failed' : 'ready',
    durationMs: null,
    width: null,
    height: null,
    category: null,
    progressPct: null,
    transcriptRunning: false,
    transcriptLanguage: null,
    orgName: null,
    viewCount: null,
    participantCount: null,
    commentCount: null,
    chapterCount: null,
    description: null,
    tags: null,
  }
}

/**
 * Capítulos de uma gravação. A `main` ainda não tem rota de capítulos: devolve
 * `null` («o servidor não diz»), e o painel e o leitor não desenham a secção.
 * Com o contrato g4 passa a pedir a rota de capítulos e a converter `at_secs` → `tMs`.
 */
export async function loadChapters(_rec: RecordingView, _signal?: AbortSignal): Promise<ChapterView[] | null> {
  return null
}

/**
 * Segmentos da transcrição com instante relativo ao vídeo. Hoje não há: a
 * transcrição que existe são as notas da SALA, com horas do relógio da
 * reunião (`RecordingNotes`), que não se podem usar para saltar o vídeo.
 */
export async function loadSegments(_rec: RecordingView, _signal?: AbortSignal): Promise<SegmentView[] | null> {
  return null
}
