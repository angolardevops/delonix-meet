/**
 * CAMADA DE MAPEAMENTO — o ÚNICO sítio da UI de gravações que lê campos da API.
 *
 * Os componentes (biblioteca, painel, leitor) só conhecem `RecordingView`. A
 * fonte é a `RecordingLibraryItem` do servidor (`GET /api/recordings` e
 * `GET /api/recordings/{id}`): duração e resolução medidas com ffprobe,
 * categoria, estados, contagens, descrição, etiquetas e publicação. Os
 * sub-recursos (capítulos, transcrição) lêem-se em `recordingLoaders.ts`.
 *
 * `null` quer dizer «o servidor não diz» (ex.: um ficheiro cuja duração o
 * ffprobe não conseguiu medir), e o ecrã desenha a estrutura sem valor —
 * nunca um número inventado.
 */
import type { RecordingLibraryItem } from '../../api'

export type SessionCategory = 'training' | 'hybrid' | 'broadcast' | 'meeting'

/**
 * Fase da gravação. Não há «a processar»: o servidor só cria a linha depois de
 * o ficheiro estar composto.
 */
export type Pipeline = 'transcribing' | 'ready' | 'published' | 'failed'

export interface RecordingView {
  /** Item original — só para o passar às funções da API (descarregar, partilhar). */
  source: RecordingLibraryItem
  id: string
  name: string
  /** Nome como o servidor o guarda (é o que se renomeia). */
  filename: string
  roomCode: string
  uploaderName: string
  createdAt: string
  /** `null` numa falhada: não há ficheiro, e «0 MB» leria-se como ficheiro vazio. */
  sizeBytes: number | null
  owned: boolean
  shareCount: number
  canDownload: boolean
  canManage: boolean
  failed: boolean
  failureReason: string | null
  pipeline: Pipeline
  published: boolean
  hasThumbnail: boolean
  snippet: string | null
  durationMs: number | null
  width: number | null
  height: number | null
  category: SessionCategory | null
  progressPct: number | null
  transcriptRunning: boolean
  transcriptReady: boolean
  transcriptLanguage: string | null
  orgName: string | null
  viewCount: number | null
  participantCount: number | null
  commentCount: number | null
  chapterCount: number | null
  captionLanguages: string[]
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

const CATEGORIES: SessionCategory[] = ['training', 'hybrid', 'broadcast', 'meeting']

export function fromRecordingItem(r: RecordingLibraryItem): RecordingView {
  const failed = r.status === 'failed'
  const published = !failed && (r.state === 'published' || r.visibility === 'org')
  return {
    source: r,
    id: r.id,
    name: displayName(r.filename),
    filename: r.filename,
    roomCode: r.room_code,
    uploaderName: r.uploader_name,
    createdAt: r.created_at,
    sizeBytes: failed ? null : r.size_bytes,
    owned: r.owned,
    shareCount: r.share_count,
    canDownload: r.can_download,
    canManage: r.can_manage,
    failed,
    failureReason: r.failure_reason,
    pipeline: failed ? 'failed' : r.status === 'transcribing' ? 'transcribing' : published ? 'published' : 'ready',
    published,
    hasThumbnail: r.has_thumbnail,
    snippet: r.snippet ?? null,
    durationMs: r.duration_ms,
    width: r.width,
    height: r.height,
    category: CATEGORIES.includes(r.kind) ? r.kind : null,
    progressPct: r.progress_pct,
    transcriptRunning: r.transcript_status === 'transcribing',
    transcriptReady: r.transcript_status === 'ready',
    transcriptLanguage: r.transcript_language,
    orgName: r.uploader_org_name,
    viewCount: r.view_count,
    participantCount: r.participant_count,
    commentCount: r.comment_count,
    chapterCount: r.chapter_count,
    captionLanguages: r.caption_languages,
    description: r.description,
    tags: r.tags,
  }
}
