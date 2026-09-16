/**
 * Leitor em página inteira — DelonixPlayer do template. Rota `#/recordings/<id>`.
 *
 * Disposição do template: vídeo de 432 px com barra própria (progresso com
 * marcas de capítulo, reproduzir, seguinte, som, tempo, velocidade, janela
 * flutuante, ecrã inteiro), título, autor e acções, o cartão de separadores
 * (Descrição · Transcrição · Participantes · Anexos) ao lado do cartão de
 * capítulos, e a coluna «A seguir» / «Da mesma série» com o cartão «Esta
 * gravação é editável».
 *
 * Os componentes só vêem `RecordingView` (`recordings/recordingView.ts`). O
 * que tem dado hoje: o ficheiro (duração e resolução MEDIDAS no browser,
 * cenas por tempo tiradas dele), autor, data, sala e tamanho da biblioteca,
 * a descrição da reunião da sala, as notas da sala, os quadros e o chat, a
 * série pela recorrência da reunião. O que espera pelo contrato de metadados
 * — legendas e «CC», menu de qualidade, capítulos do servidor, organização,
 * visualizações, etiquetas, participantes, comentários, «Publicadas» e os
 * selos de duração/resolução nas listas — tem o sítio pronto e só aparece
 * quando a camada de mapeamento o devolver. «Guardar em…» não tem servidor.
 */
import { useCallback, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  ChatHistoryMsg,
  downloadRecording,
  isAbort,
  listMeetings,
  listWhiteboards,
  Meeting,
  recordingsLibrary,
  roomChatHistory,
  WhiteboardMeta,
} from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { Icon, IconName } from '../ui/icons'
import { Alert, Avatar, Button, cx, Dialog, Empty, Skeleton, Spinner, Tabs, TextInput } from '../ui/kit'
import '../ui/player.css'
import '../ui/recordings.css'
import { useBoardPng } from './boards/useBoardPng'
import ChapterList from './recordings/ChapterList'
import { formatBytes, formatDate, formatDateTime, formatDateTimeShort, formatDayMonth, thumbBackground } from './recordings/format'
import { chapterAt, formatClock, resolutionLabel, visibleState } from './recordings/libraryData'
import { clockPair, ProgressBar, usePlayback } from './recordings/playback'
import { frameTimes, nextUp, sameSeries } from './recordings/playerData'
import { useFrameGrabs, useRecordingVideo } from './recordings/recordingMedia'
import RecordingNotes from './recordings/RecordingNotes'
import RecordingState from './recordings/RecordingState'
import { ChapterView, fromRecordingItem, loadChapters, loadSegments, RecordingView } from './recordings/recordingView'
import ShareDialog from './recordings/ShareDialog'
import { playerHash, studioEditHash } from './recordings/studioLink'
import Transcript from './recordings/Transcript'

type InfoTab = 'description' | 'transcript' | 'participants' | 'attachments'
type SideTab = 'next' | 'series'

const RATES = [1, 1.25, 1.5, 2, 0.75]

const optional = <T,>(p: Promise<T>, fallback: T): Promise<T> =>
  p.catch((e) => {
    if (isAbort(e)) throw e
    return fallback
  })

export default function RecordingPlayer({ id }: { id: string }) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const { state, reload } = useAsync(async (signal) => {
    const [library, meetings] = await Promise.all([recordingsLibrary(signal), optional(listMeetings(signal), [] as Meeting[])])
    return { library: library.map(fromRecordingItem), meetings }
  }, [])
  const count = state.s === 'ready' ? state.d.library.length : null

  return (
    <div className="pl-root dx-stage">
      <PageBar title={t('player.biblioteca')}>
        <form
          className="pl-search"
          role="search"
          onSubmit={(e) => {
            e.preventDefault()
            const q = query.trim()
            location.hash = q ? `/recordings?q=${encodeURIComponent(q)}` : '/recordings'
          }}
        >
          <TextInput
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={count === null ? t('player.pesquisar') : t('player.pesquisarEm', { count })}
            aria-label={t('recordings.pesquisa.rotulo')}
          />
        </form>
      </PageBar>
      <AsyncSection
        state={state}
        onRetry={reload}
        skeleton={
          <div className="pl-layout" aria-busy="true">
            <div className="pl-main">
              <Skeleton h={432} />
              <Skeleton h={200} />
            </div>
          </div>
        }
      >
        {({ library, meetings }) => {
          const rec = library.find((r) => r.id === id)
          if (!rec) {
            return (
              <div className="page">
                <Empty
                  icon="film"
                  title={t('player.naoEncontrada')}
                  action={
                    <Button size="sm" variant="secondary" onClick={() => (location.hash = '/recordings')}>
                      {t('player.voltar')}
                    </Button>
                  }
                >
                  {t('player.naoEncontradaTexto')}
                </Empty>
              </div>
            )
          }
          return <Player key={rec.id} rec={rec} library={library} meetings={meetings} onChanged={reload} />
        }}
      </AsyncSection>
    </div>
  )
}

function Player({ rec, library, meetings, onChanged }: { rec: RecordingView; library: RecordingView[]; meetings: Meeting[]; onChanged: () => void }) {
  const { t, i18n } = useTranslation()
  const videoRef = useRef<HTMLVideoElement>(null)
  const stageRef = useRef<HTMLDivElement>(null)
  const [video, retry] = useRecordingVideo(rec, !rec.failed)
  const src = video.s === 'ready' ? video.url : null
  const pb = usePlayback(videoRef, src)
  const [info, setInfo] = useState<InfoTab>('description')
  const [side, setSide] = useState<SideTab>('next')
  const [share, setShare] = useState(false)
  const [downloading, setDownloading] = useState(false)
  const [actionErr, setActionErr] = useState('')
  const pipAvailable = typeof document !== 'undefined' && document.pictureInPictureEnabled === true

  const meeting = meetings.find((m) => m.room_code === rec.roomCode) ?? null
  const byId = useMemo(() => new Map(library.map((r) => [r.id, r])), [library])
  const sources = useMemo(() => library.map((r) => r.source), [library])
  const upNext = useMemo(() => nextUp(sources, rec.id).map((s) => byId.get(s.id)!), [sources, rec.id, byId])
  const series = useMemo(() => sameSeries(sources, meetings, rec.source).map((s) => byId.get(s.id)!), [sources, meetings, rec.source, byId])
  const next = upNext[0] ?? null

  const extra = useAsync(async (signal) => {
    const [chapters, segments, boards, chat] = await Promise.all([
      loadChapters(rec, signal),
      loadSegments(rec, signal),
      optional(listWhiteboards(signal), null as WhiteboardMeta[] | null),
      // O chat não aceita AbortSignal: um erro (403/404) é «sem acesso», não um ecrã partido.
      roomChatHistory(rec.roomCode).catch(() => null as ChatHistoryMsg[] | null),
    ])
    return { chapters, segments, boards: boards?.filter((b) => b.room_code === rec.roomCode) ?? null, chat }
  }, [rec.id])
  const x = extra.state.s === 'ready' ? extra.state.d : null
  const chapters = x?.chapters ?? null
  const attachCount = x ? (x.boards?.length ?? 0) + (x.chat?.length ?? 0) : null

  const durationMs = rec.durationMs ?? pb.durationMs
  const res = resolutionLabel(rec.height !== null ? rec : pb.size)

  // Sem capítulos do servidor: cenas por tempo, tiradas do próprio ficheiro.
  const scenes = useMemo<ChapterView[]>(() => {
    if (!pb.durationMs) return []
    const times = frameTimes(pb.durationMs / 1000)
    const step = pb.durationMs / Math.max(1, times.length)
    return times.map((_, i) => ({ id: `cena-${i}`, tMs: Math.round(i * step), title: t('player.cena', { n: i + 1 }), auto: true }))
  }, [pb.durationMs, t])
  const listed = chapters && chapters.length > 0 ? chapters : scenes
  const grabTimes = useMemo(() => {
    if (!pb.durationMs || listed.length === 0) return []
    return listed.map((c, i) => {
      const end = listed[i + 1]?.tMs ?? pb.durationMs!
      return (c.tMs + Math.min(end - c.tMs, 8000) / 2) / 1000
    })
  }, [listed, pb.durationMs])
  const { frames, failed: framesFailed } = useFrameGrabs(src, grabTimes)

  const seek = useCallback((ms: number) => (src ? pb.seek(ms, true) : pb.queueSeek(ms)), [src, pb])

  async function download() {
    setDownloading(true)
    setActionErr('')
    try {
      await downloadRecording(rec.source)
    } catch (e) {
      setActionErr(apiErrorMessage(e, t('recordings.accoes.erroDescarregar')))
    } finally {
      setDownloading(false)
    }
  }

  async function fullscreen() {
    const el = stageRef.current
    if (!el) return
    try {
      if (document.fullscreenElement) await document.exitFullscreen()
      else await el.requestFullscreen()
    } catch {
      setActionErr(t('player.ecraInteiroRecusado'))
    }
  }

  async function pip() {
    try {
      await videoRef.current?.requestPictureInPicture()
    } catch {
      setActionErr(t('recordings.leitor.pipRecusada'))
    }
  }

  const goNext = () => next && (location.hash = playerHash(next.id).slice(1))
  const rateLabel = new Intl.NumberFormat(i18n.language, { minimumFractionDigits: 1, maximumFractionDigits: 2 }).format(pb.rate)
  const byline = [rec.orgName, formatDate(rec.createdAt, i18n.language), t('recordings.salaCodigo', { code: rec.roomCode }), rec.sizeBytes !== null ? formatBytes(rec.sizeBytes, i18n.language) : null, rec.viewCount !== null ? t('player.visualizacoes', { count: rec.viewCount }) : null].filter(Boolean)

  const tabs: { value: InfoTab; label: string }[] = [
    { value: 'description', label: t('player.descricao') },
    { value: 'transcript', label: t('player.transcricao') },
  ]
  if (rec.participantCount !== null) tabs.push({ value: 'participants', label: t('player.participantesN', { count: rec.participantCount }) })
  tabs.push({ value: 'attachments', label: attachCount === null ? t('player.anexos') : t('player.anexosN', { count: attachCount }) })

  return (
    <div className="pl-layout">
      <div className="pl-main">
        {rec.failed ? (
          <Alert tone="danger" icon="alert">
            {rec.failureReason || t('recordings.estado.semCausa')}
          </Alert>
        ) : (
          <div className="pl-video" ref={stageRef}>
            {src ? (
              <video ref={videoRef} className="pl-video__el" src={src} autoPlay playsInline onClick={pb.toggle} />
            ) : (
              <div className="pl-video__poster" style={{ background: thumbBackground(rec.name) }}>
                {video.s === 'error' ? (
                  <div className="rec-player__error" role="alert">
                    <Icon name="alert" />
                    <span>{t('recordings.leitor.erroVideo')}</span>
                    <Button size="sm" variant="secondary" icon="refresh" onClick={retry}>
                      {t('ui.tentarDeNovo')}
                    </Button>
                  </div>
                ) : (
                  <Spinner label={t('recordings.leitor.aCarregar')} />
                )}
              </div>
            )}
            {res && (
              <div className="pl-badges">
                <span className="rec-badge is-soft" title={t('recordings.leitor.resolucao')}>
                  {res === '4K' ? '2160p' : res}
                </span>
              </div>
            )}
            {pb.ended && next && (
              <div className="pl-ended" role="status">
                <span className="dx-eyebrow">{t('player.aSeguir')}</span>
                <strong>{next.name}</strong>
                <div className="pl-ended__actions">
                  <Button size="sm" variant="primary" icon="play" onClick={goNext}>
                    {t('player.verAgora')}
                  </Button>
                  <Button size="sm" variant="ghost" icon="refresh" onClick={() => pb.seek(0, true)}>
                    {t('player.repetir')}
                  </Button>
                </div>
              </div>
            )}
            <div className="pl-controls" role="group" aria-label={t('player.transporte')}>
              <ProgressBar
                size="full"
                nowMs={pb.nowMs}
                durationMs={durationMs}
                ticksMs={chapters?.map((c) => c.tMs) ?? []}
                onSeek={seek}
                label={t('recordings.leitor.progresso')}
                valueText={clockPair(pb.nowMs, durationMs)}
              />
              <div className="pl-controls__row">
                <Ctl icon={pb.playing ? 'pause' : 'play'} label={pb.playing ? t('player.pausar') : t('player.reproduzir')} onClick={pb.toggle} disabled={!src} />
                <Ctl icon="chevronRight" label={t('player.seguinte')} onClick={goNext} disabled={!next} />
                <Ctl icon="volume" label={pb.muted ? t('player.ligarSom') : t('player.silenciar')} pressed={pb.muted} onClick={() => pb.setMuted(!pb.muted)} disabled={!src} />
                <span className="pl-controls__time dx-num">{clockPair(pb.nowMs, durationMs)}</span>
                <span className="dx-spacer" />
                <button
                  type="button"
                  className="pl-controls__rate dx-num"
                  aria-label={t('player.velocidade', { rate: rateLabel })}
                  disabled={!src}
                  onClick={() => pb.setRate(RATES[(RATES.indexOf(pb.rate) + 1) % RATES.length])}
                >
                  {rateLabel}×
                </button>
                {pipAvailable && <Ctl icon="pip" label={t('recordings.leitor.pip')} onClick={() => void pip()} disabled={!src} boxed />}
                <Ctl icon="maximize" label={t('player.ecraInteiro')} onClick={() => void fullscreen()} boxed />
              </div>
            </div>
          </div>
        )}

        <div className="pl-head">
          <h2 className="pl-title">{rec.name}</h2>
          <div className="pl-byline">
            <div className="pl-author">
              <Avatar name={rec.uploaderName} size={32} />
              <div>
                <div className="pl-author__name">{rec.uploaderName}</div>
                <div className="pl-author__meta">{byline.join(' · ')}</div>
              </div>
            </div>
            <div className="dx-spacer" />
            {!rec.failed && (
              <div className="pl-actions">
                <Button variant="primary" size="sm" icon="scissors" onClick={() => (location.hash = studioEditHash(rec.id).slice(1))}>
                  {t('player.editarStudio')}
                </Button>
                {rec.owned && (
                  <Button size="sm" variant="secondary" onClick={() => setShare(true)}>
                    {rec.shareCount > 0 ? t('recordings.accoes.partilharN', { count: rec.shareCount }) : t('recordings.accoes.partilhar')}
                  </Button>
                )}
                {rec.canDownload && (
                  <Button size="sm" variant="secondary" busy={downloading} onClick={() => void download()}>
                    {t('recordings.accoes.descarregar')}
                  </Button>
                )}
              </div>
            )}
          </div>
          {actionErr && <Alert tone="danger">{actionErr}</Alert>}
        </div>

        <div className={cx('pl-lower', rec.failed && 'is-single')}>
          <section className="pl-card pl-info">
            <Tabs<InfoTab> label={t('player.separadores')} value={info} onChange={setInfo} tabs={tabs} />
            <div className="pl-info__body">
              {info === 'description' && <Description rec={rec} meeting={meeting} />}
              {info === 'transcript' && (x?.segments ? <Transcript segments={x.segments} nowMs={pb.nowMs} onSeek={seek} /> : <RecordingNotes roomCode={rec.roomCode} />)}
              {info === 'attachments' && (
                <AsyncSection state={extra.state} onRetry={extra.reload}>
                  {(d) => <Attachments boards={d.boards} chat={d.chat} />}
                </AsyncSection>
              )}
            </div>
          </section>

          {!rec.failed && (
            <section className="pl-card pl-chapters" aria-labelledby="pl-chapters-title">
              <div className="pl-chapters__head">
                <h3 id="pl-chapters-title">{chapters && chapters.length > 0 ? t('recordings.capitulos.titulo') : t('player.cenas')}</h3>
                <span className="dx-num">{chapters && chapters.length > 0 ? (chapters.every((c) => c.auto) ? t('player.automaticos') : t('player.manuais')) : t('player.cenasAjuda')}</span>
              </div>
              {listed.length === 0 ? (
                <p className="pl-empty">{video.s === 'error' ? t('player.cenasErro') : t('player.cenasAEsperar')}</p>
              ) : (
                <>
                  {framesFailed && <p className="pl-empty">{t('player.cenasErro')}</p>}
                  <ChapterList chapters={listed} active={chapterAt(listed, pb.nowMs)} onSeek={seek} variant="thumbs" frames={frames} />
                </>
              )}
            </section>
          )}
        </div>
      </div>

      <aside className="pl-side" aria-label={t('player.lista')}>
        <div className="pl-side__tabs" role="tablist" aria-label={t('player.lista')}>
          <button type="button" role="tab" aria-selected={side === 'next'} onClick={() => setSide('next')}>
            {t('player.aSeguir')}
          </button>
          <button type="button" role="tab" aria-selected={side === 'series'} onClick={() => setSide('series')}>
            {t('player.mesmaSerie')}
            {series.length > 0 && <span className="dx-num"> · {series.length}</span>}
          </button>
        </div>
        <RecList items={side === 'next' ? upNext : series} empty={side === 'next' ? t('player.semSeguintes') : meeting ? t('player.semSerie') : t('player.semReuniao')} />
        {!rec.failed && (
          <div className="pl-editable">
            <strong>{t('player.editavelTitulo')}</strong>
            <p>{t('player.editavelTexto')}</p>
          </div>
        )}
      </aside>

      {share && (
        <ShareDialog
          rec={rec.source}
          onClose={() => {
            setShare(false)
            onChanged()
          }}
        />
      )}
    </div>
  )
}

function Ctl({ icon, label, onClick, disabled, pressed, boxed }: { icon: IconName; label: string; onClick: () => void; disabled?: boolean; pressed?: boolean; boxed?: boolean }) {
  return (
    <button type="button" className={cx('pl-ctl', boxed && 'is-boxed')} aria-label={label} title={label} aria-pressed={pressed} disabled={disabled} onClick={onClick}>
      <Icon name={icon} size={boxed ? 12 : 15} />
    </button>
  )
}

function RecList({ items, empty }: { items: RecordingView[]; empty: string }) {
  const { i18n } = useTranslation()
  if (items.length === 0) return <p className="pl-empty">{empty}</p>
  return (
    <ul className="pl-list">
      {items.map((r) => {
        const res = resolutionLabel(r)
        return (
          <li key={r.id}>
            <a className="pl-item" href={playerHash(r.id)}>
              <span className="pl-item__thumb" style={{ background: thumbBackground(r.name) }} aria-hidden="true">
                {res && <span className="rec-badge pl-item__res">{res}</span>}
                {r.durationMs !== null && <span className="rec-badge is-soft pl-item__dur dx-num">{formatClock(r.durationMs)}</span>}
              </span>
              <span className="pl-item__text">
                <span className="pl-item__title">{r.name}</span>
                <span className="pl-item__meta dx-num">
                  {r.uploaderName} · {formatDayMonth(r.createdAt, i18n.language)}
                </span>
                <span className="pl-item__state dx-num">
                  <RecordingState state={visibleState(r)} lower />
                </span>
              </span>
            </a>
          </li>
        )
      })}
    </ul>
  )
}

/**
 * Descrição: a da gravação quando a camada de mapeamento a trouxer; até lá, a
 * da reunião agendada cuja sala é a da gravação. Etiquetas por baixo.
 */
function Description({ rec, meeting }: { rec: RecordingView; meeting: Meeting | null }) {
  const { t, i18n } = useTranslation()
  const tags = rec.tags && rec.tags.length > 0 && (
    <ul className="pl-tags">
      {rec.tags.map((tag) => (
        <li key={tag}>#{tag}</li>
      ))}
    </ul>
  )
  if (rec.description !== null) {
    return (
      <div className="pl-desc">
        {rec.description.trim() ? <p className="pl-desc__text">{rec.description}</p> : <p className="pl-empty">{t('player.semDescricaoGravacao')}</p>}
        {tags}
      </div>
    )
  }
  if (!meeting) return <p className="pl-empty">{t('player.semReuniaoDescricao', { code: rec.roomCode })}</p>
  return (
    <div className="pl-desc">
      {meeting.description.trim() ? <p className="pl-desc__text">{meeting.description}</p> : <p className="pl-empty">{t('player.semDescricao')}</p>}
      <p className="pl-desc__meta dx-num">
        {meeting.title} · {formatDateTime(meeting.starts_at, i18n.language)} · {t('player.duracaoMin', { count: meeting.duration_min })} · {t('player.organizador', { nome: meeting.owner_name })}
        {(meeting.recurrence_freq || meeting.recurrence_parent_id) && ` · ${t('player.recorrente')}`}
      </p>
      {tags}
    </div>
  )
}

function Attachments({ boards, chat }: { boards: WhiteboardMeta[] | null; chat: ChatHistoryMsg[] | null }) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState<WhiteboardMeta | null>(null)
  return (
    <div className="pl-attach">
      <section aria-labelledby="pl-boards">
        <h3 id="pl-boards" className="pl-attach__title">
          {t('player.quadros')}
          {boards && <span className="dx-num dx-muted"> · {boards.length}</span>}
        </h3>
        {boards === null ? (
          <p className="pl-empty">{t('player.quadrosErro')}</p>
        ) : boards.length === 0 ? (
          <p className="pl-empty">{t('player.semQuadros')}</p>
        ) : (
          <ul className="pl-boards">
            {boards.map((b) => (
              <li key={b.id}>
                <BoardThumb board={b} onOpen={() => setOpen(b)} />
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-labelledby="pl-chat">
        <h3 id="pl-chat" className="pl-attach__title">
          {t('player.chat')}
          {chat && <span className="dx-num dx-muted"> · {chat.length}</span>}
        </h3>
        {chat === null ? (
          <p className="pl-empty">{t('player.chatSemAcesso')}</p>
        ) : chat.length === 0 ? (
          <p className="pl-empty">{t('player.semChat')}</p>
        ) : (
          <ol className="pl-chat">
            {chat.map((m) => (
              <li key={m.id}>
                <span className="pl-chat__who">{m.username}</span>
                <span className="pl-chat__time dx-num">{formatDateTimeShort(m.created_at, i18n.language)}</span>
                <span className="pl-chat__msg">{m.message}</span>
              </li>
            ))}
          </ol>
        )}
      </section>
      {open && <BoardDialog board={open} onClose={() => setOpen(null)} />}
    </div>
  )
}

function BoardThumb({ board, onOpen }: { board: WhiteboardMeta; onOpen: () => void }) {
  const { t, i18n } = useTranslation()
  const png = useBoardPng(board.id)
  const title = board.title || t('boards.semTitulo')
  return (
    <button type="button" className="pl-board" onClick={onOpen} aria-label={t('player.abrirQuadro', { title })}>
      <span className="pl-board__img">{png.s === 'ready' ? <img src={png.url} alt="" /> : png.s === 'loading' ? <Spinner /> : <Icon name="board" />}</span>
      <span className="pl-board__title">{title}</span>
      <span className="pl-board__meta dx-num">{formatDateTimeShort(board.created_at, i18n.language)}</span>
    </button>
  )
}

function BoardDialog({ board, onClose }: { board: WhiteboardMeta; onClose: () => void }) {
  const { t } = useTranslation()
  const png = useBoardPng(board.id)
  return (
    <Dialog title={board.title || t('boards.semTitulo')} onClose={onClose} wide>
      <div className="pl-board-view">
        {png.s === 'ready' ? <img src={png.url} alt={board.title || t('boards.semTitulo')} /> : png.s === 'loading' ? <Spinner /> : <Alert tone="danger">{t('player.quadroErro')}</Alert>}
      </div>
    </Dialog>
  )
}

