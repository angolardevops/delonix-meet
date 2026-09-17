/**
 * Leitor em página inteira — DelonixPlayer do template. Rota `#/recordings/<id>`.
 *
 * Do template, o que tem dado real por trás:
 *  - vídeo com os controlos nativos (reproduzir, volume, tempo, velocidade,
 *    ecrã inteiro) e «Seguinte», que salta para a próxima da biblioteca;
 *  - miniaturas por tempo, tiradas do próprio ficheiro no browser, com a
 *    actual realçada — NÃO são capítulos: o servidor não tem capítulos, e
 *    por isso não há marcas na barra nem títulos inventados;
 *  - título, autor, data, sala e tamanho vêm da biblioteca;
 *  - «Editar no Studio» navega para `#/studio?editar=<id>` (contrato em
 *    `recordings/studioLink.ts`); Partilhar e Descarregar com as regras do
 *    servidor;
 *  - Descrição: a reunião cuja sala é a da gravação (`/api/meetings`);
 *  - Transcrição: as notas da sala (`/api/rooms/{room_code}/minutes`);
 *  - Anexos: os quadros guardados com essa sala e o histórico do chat;
 *  - «A seguir» (biblioteca) e «Da mesma série» (`recurrence_parent_id`).
 *
 * Fica de fora, sem botão nem número: pesquisa em todas as transcrições,
 * legendas sobre o vídeo e «CC», menu de qualidade, capítulos, visualizações,
 * organização do autor, «Guardar em…», participantes, etiquetas, comentários,
 * duração/resolução/estado nos itens de «A seguir», «Publicadas» e o cartão
 * «Esta gravação é editável» (faixas separadas) — nada disso existe no
 * servidor.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  ChatHistoryMsg,
  downloadRecording,
  isAbort,
  listMeetings,
  listWhiteboards,
  Meeting,
  RecordingItem,
  recordingObjectUrl,
  recordingsLibrary,
  roomChatHistory,
  WhiteboardMeta,
} from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { Icon } from '../ui/icons'
import { Alert, Avatar, Button, cx, Dialog, Empty, Skeleton, Spinner, Tabs } from '../ui/kit'
import '../ui/player.css'
import '../ui/recordings.css'
import { useBoardPng } from './boards/useBoardPng'
import { formatBytes, formatDate, formatDateTime, formatDateTimeShort, formatDuration, isFailed, recordingName, thumbBackground } from './recordings/format'
import { frameTimes, nextUp, sameSeries } from './recordings/playerData'
import RecordingNotes from './recordings/RecordingNotes'
import ShareDialog from './recordings/ShareDialog'
import { playerHash, studioEditHash } from './recordings/studioLink'

type Video = { s: 'loading' } | { s: 'ready'; url: string } | { s: 'error' }
type InfoTab = 'description' | 'transcript' | 'attachments'
type SideTab = 'next' | 'series'

const optional = <T,>(p: Promise<T>, fallback: T): Promise<T> =>
  p.catch((e) => {
    if (isAbort(e)) throw e
    return fallback
  })

export default function RecordingPlayer({ id }: { id: string }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync(
    async (signal) => {
      const [library, meetings] = await Promise.all([recordingsLibrary(signal), optional(listMeetings(signal), [] as Meeting[])])
      return { library, meetings }
    },
    [],
  )

  return (
    <div className="pl-root dx-stage">
      <PageBar title={t('player.biblioteca')}>
        <Button size="sm" variant="ghost" icon="list" onClick={() => (location.hash = '/recordings')}>
          {t('player.voltar')}
        </Button>
      </PageBar>
      <AsyncSection
        state={state}
        onRetry={reload}
        skeleton={
          <div className="page pl-layout" aria-busy="true">
            <Skeleton h={360} />
            <Skeleton h={200} />
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
          return <Player rec={rec} library={library} meetings={meetings} onChanged={reload} />
        }}
      </AsyncSection>
    </div>
  )
}

function Player({ rec, library, meetings, onChanged }: { rec: RecordingItem; library: RecordingItem[]; meetings: Meeting[]; onChanged: () => void }) {
  const { t, i18n } = useTranslation()
  const videoRef = useRef<HTMLVideoElement>(null)
  const [video, setVideo] = useState<Video>({ s: 'loading' })
  const [attempt, setAttempt] = useState(0)
  const [duration, setDuration] = useState<number | null>(null)
  const [size, setSize] = useState<{ w: number; h: number } | null>(null)
  const [now, setNow] = useState(0)
  const [ended, setEnded] = useState(false)
  const [info, setInfo] = useState<InfoTab>('description')
  const [side, setSide] = useState<SideTab>('next')
  const [share, setShare] = useState(false)
  const [downloading, setDownloading] = useState(false)
  const [actionErr, setActionErr] = useState('')
  const failed = isFailed(rec)
  const name = recordingName(rec)
  const meeting = meetings.find((m) => m.room_code === rec.room_code) ?? null
  const upNext = useMemo(() => nextUp(library, rec.id), [library, rec.id])
  const series = useMemo(() => sameSeries(library, meetings, rec), [library, meetings, rec])
  const next = upNext[0] ?? null

  // Nesta página a pessoa escolheu ver: o vídeo carrega logo.
  useEffect(() => {
    if (failed) return
    let live = true
    let made = ''
    setVideo({ s: 'loading' })
    recordingObjectUrl(rec)
      .then((u) => {
        if (live) {
          made = u
          setVideo({ s: 'ready', url: u })
        } else URL.revokeObjectURL(u)
      })
      .catch(() => live && setVideo({ s: 'error' }))
    return () => {
      live = false
      if (made) URL.revokeObjectURL(made)
    }
  }, [rec, failed, attempt])

  function onMetadata() {
    const v = videoRef.current
    if (!v) return
    if (v.videoWidth && v.videoHeight) setSize({ w: v.videoWidth, h: v.videoHeight })
    if (Number.isFinite(v.duration)) setDuration(v.duration)
    else {
      // WebM do MediaRecorder sem duração no cabeçalho: medir e voltar ao início.
      const onChange = () => {
        if (!Number.isFinite(v.duration)) return
        v.removeEventListener('durationchange', onChange)
        setDuration(v.duration)
        v.currentTime = 0
      }
      v.addEventListener('durationchange', onChange)
      v.currentTime = Number.MAX_SAFE_INTEGER
    }
  }

  async function download() {
    setDownloading(true)
    setActionErr('')
    try {
      await downloadRecording(rec)
    } catch (e) {
      setActionErr(apiErrorMessage(e, t('recordings.accoes.erroDescarregar')))
    } finally {
      setDownloading(false)
    }
  }

  const seek = (s: number) => {
    const v = videoRef.current
    if (!v) return
    v.currentTime = s
    void v.play().catch(() => undefined)
  }

  return (
    <div className="page pl-layout">
      <div className="pl-main">
        {failed ? (
          <Alert tone="danger" icon="alert">
            {rec.failure_reason || t('recordings.estado.semCausa')}
          </Alert>
        ) : (
          <div className="rec-player pl-video">
            {video.s === 'ready' ? (
              <video
                ref={videoRef}
                className="rec-player__video"
                src={video.url}
                controls
                autoPlay
                playsInline
                onLoadedMetadata={onMetadata}
                onTimeUpdate={(e) => setNow(e.currentTarget.currentTime)}
                onPlay={() => setEnded(false)}
                onEnded={() => setEnded(true)}
              />
            ) : (
              <div className="rec-player__poster" style={{ background: thumbBackground(rec.filename) }}>
                {video.s === 'loading' ? (
                  <Spinner label={t('recordings.leitor.aCarregar')} />
                ) : (
                  <div className="rec-player__error" role="alert">
                    <Icon name="alert" />
                    <span>{t('recordings.leitor.erroVideo')}</span>
                    <Button size="sm" variant="secondary" icon="refresh" onClick={() => setAttempt((n) => n + 1)}>
                      {t('ui.tentarDeNovo')}
                    </Button>
                  </div>
                )}
              </div>
            )}
            {size && (
              <div className="rec-player__badges pl-badges">
                <span className="rec-player__badge dx-num" title={t('recordings.leitor.resolucao')}>
                  {size.h >= 2160 ? '2160p' : `${size.h}p`}
                </span>
              </div>
            )}
            {ended && next && (
              <div className="pl-ended" role="status">
                <span className="dx-eyebrow">{t('player.aSeguir')}</span>
                <strong>{recordingName(next)}</strong>
                <div className="pl-ended__actions">
                  <Button size="sm" variant="primary" icon="play" onClick={() => (location.hash = playerHash(next.id).slice(1))}>
                    {t('player.verAgora')}
                  </Button>
                  <Button size="sm" variant="ghost" icon="refresh" onClick={() => seek(0)}>
                    {t('player.repetir')}
                  </Button>
                </div>
              </div>
            )}
          </div>
        )}

        {!failed && (
          <div className="pl-transport" role="group" aria-label={t('player.transporte')}>
            <Button size="sm" variant="ghost" icon="play" disabled={!next} onClick={() => next && (location.hash = playerHash(next.id).slice(1))}>
              {t('player.seguinte')}
            </Button>
            {duration !== null && (
              <span className="dx-num dx-muted">
                {formatDuration(now)} / {formatDuration(duration)}
              </span>
            )}
          </div>
        )}

        <div className="pl-head">
          <h2 className="pl-title">{name}</h2>
          <div className="pl-byline">
            <div className="pl-author">
              <Avatar name={rec.uploader_name} size={32} />
              <div>
                <div className="pl-author__name">{rec.uploader_name}</div>
                <div className="pl-author__meta dx-num">
                  {formatDate(rec.created_at, i18n.language)} · {t('recordings.salaCodigo', { code: rec.room_code })}
                  {!failed && ` · ${formatBytes(rec.size_bytes, i18n.language)}`}
                </div>
              </div>
            </div>
            <div className="dx-spacer" />
            {!failed && (
              <div className="pl-actions">
                <Button variant="primary" size="sm" icon="scissors" onClick={() => (location.hash = studioEditHash(rec.id).slice(1))}>
                  {t('player.editarStudio')}
                </Button>
                {rec.owned && (
                  <Button size="sm" onClick={() => setShare(true)}>
                    {rec.share_count > 0 ? t('recordings.accoes.partilharN', { count: rec.share_count }) : t('recordings.accoes.partilhar')}
                  </Button>
                )}
                {rec.can_download && (
                  <Button size="sm" busy={downloading} onClick={() => void download()}>
                    {t('recordings.accoes.descarregar')}
                  </Button>
                )}
              </div>
            )}
          </div>
          {actionErr && <Alert tone="danger">{actionErr}</Alert>}
        </div>

        <div className={cx('pl-lower', failed && 'is-single')}>
          <section className="pl-card pl-info">
            <Tabs<InfoTab>
              label={t('player.separadores')}
              value={info}
              onChange={setInfo}
              tabs={[
                { value: 'description', label: t('player.descricao') },
                { value: 'transcript', label: t('player.transcricao') },
                { value: 'attachments', label: t('player.anexos') },
              ]}
            />
            <div className="pl-info__body">
              {info === 'description' && <Description meeting={meeting} roomCode={rec.room_code} />}
              {info === 'transcript' && <RecordingNotes roomCode={rec.room_code} />}
              {info === 'attachments' && <Attachments roomCode={rec.room_code} />}
            </div>
          </section>

          {!failed && video.s === 'ready' && (
            <section className="pl-card pl-frames" aria-labelledby="pl-frames-title">
              <div className="pl-frames__head">
                <h3 id="pl-frames-title">{t('player.cenas')}</h3>
                <span className="dx-muted">{t('player.cenasAjuda')}</span>
              </div>
              <Frames url={video.url} duration={duration} now={now} onSeek={seek} />
            </section>
          )}
        </div>
      </div>

      <aside className="pl-side" aria-label={t('player.lista')}>
        <div className="dx-seg pl-side__tabs" role="tablist" aria-label={t('player.lista')}>
          <button type="button" role="tab" aria-selected={side === 'next'} aria-pressed={side === 'next'} onClick={() => setSide('next')}>
            {t('player.aSeguir')}
          </button>
          <button type="button" role="tab" aria-selected={side === 'series'} aria-pressed={side === 'series'} onClick={() => setSide('series')}>
            {t('player.mesmaSerie')}
            {series.length > 0 && <span className="dx-num"> · {series.length}</span>}
          </button>
        </div>
        <RecList items={side === 'next' ? upNext : series} empty={side === 'next' ? t('player.semSeguintes') : meeting ? t('player.semSerie') : t('player.semReuniao')} />
      </aside>

      {share && (
        <ShareDialog
          rec={rec}
          onClose={() => {
            setShare(false)
            onChanged()
          }}
        />
      )}
    </div>
  )
}

function RecList({ items, empty }: { items: RecordingItem[]; empty: string }) {
  const { i18n } = useTranslation()
  if (items.length === 0) return <p className="pl-empty">{empty}</p>
  return (
    <ul className="pl-list">
      {items.map((r) => (
        <li key={r.id}>
          <a className="pl-item" href={playerHash(r.id)}>
            <span className="pl-item__thumb" style={{ background: thumbBackground(r.filename) }} aria-hidden="true">
              <Icon name="play" size={16} />
            </span>
            <span className="pl-item__text">
              <span className="pl-item__title">{recordingName(r)}</span>
              <span className="pl-item__meta dx-num">
                {r.uploader_name} · {formatDateTimeShort(r.created_at, i18n.language)}
              </span>
            </span>
          </a>
        </li>
      ))}
    </ul>
  )
}

function Description({ meeting, roomCode }: { meeting: Meeting | null; roomCode: string }) {
  const { t, i18n } = useTranslation()
  if (!meeting) return <p className="pl-empty">{t('player.semReuniaoDescricao', { code: roomCode })}</p>
  return (
    <div className="pl-desc">
      <p className="pl-desc__title">{meeting.title}</p>
      <p className="pl-desc__meta dx-num">
        {formatDateTime(meeting.starts_at, i18n.language)} · {t('player.duracaoMin', { count: meeting.duration_min })} · {t('player.organizador', { nome: meeting.owner_name })}
        {(meeting.recurrence_freq || meeting.recurrence_parent_id) && ` · ${t('player.recorrente')}`}
      </p>
      {meeting.description.trim() ? <p className="pl-desc__text">{meeting.description}</p> : <p className="pl-empty">{t('player.semDescricao')}</p>}
    </div>
  )
}

function Attachments({ roomCode }: { roomCode: string }) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState<WhiteboardMeta | null>(null)
  const { state, reload } = useAsync(
    async (signal) => {
      const [boards, chat] = await Promise.all([
        optional(listWhiteboards(signal), null as WhiteboardMeta[] | null),
        // O chat não aceita AbortSignal: um erro (403/404) é «sem acesso», não um ecrã partido.
        roomChatHistory(roomCode).catch(() => null as ChatHistoryMsg[] | null),
      ])
      return { boards: boards?.filter((b) => b.room_code === roomCode) ?? null, chat }
    },
    [roomCode],
  )
  return (
    <AsyncSection state={state} onRetry={reload}>
      {({ boards, chat }) => (
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
      )}
    </AsyncSection>
  )
}

function BoardThumb({ board, onOpen }: { board: WhiteboardMeta; onOpen: () => void }) {
  const { t, i18n } = useTranslation()
  const png = useBoardPng(board.id)
  const title = board.title || t('boards.semTitulo')
  return (
    <button type="button" className="pl-board" onClick={onOpen} aria-label={t('player.abrirQuadro', { title })}>
      <span className="pl-board__img">
        {png.s === 'ready' ? <img src={png.url} alt="" /> : png.s === 'loading' ? <Spinner /> : <Icon name="board" />}
      </span>
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

/**
 * Miniaturas por tempo, tiradas do ficheiro já carregado: um segundo `<video>`
 * escondido salta para cada instante e desenha o fotograma num canvas. Nada
 * vai para o servidor e nada fica guardado.
 */
function Frames({ url, duration, now, onSeek }: { url: string; duration: number | null; now: number; onSeek: (s: number) => void }) {
  const { t } = useTranslation()
  const times = useMemo(() => (duration ? frameTimes(duration) : []), [duration])
  const [thumbs, setThumbs] = useState<(string | null)[]>([])
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    if (times.length === 0) return
    let cancelled = false
    const v = document.createElement('video')
    v.muted = true
    v.preload = 'auto'
    v.playsInline = true
    v.src = url
    const canvas = document.createElement('canvas')
    canvas.width = 160
    canvas.height = 90
    const ctx = canvas.getContext('2d')
    setThumbs(times.map(() => null))
    setFailed(false)
    const waitFor = (ev: string) =>
      new Promise<void>((resolve, reject) => {
        const ok = () => {
          cleanup()
          resolve()
        }
        const bad = () => {
          cleanup()
          reject(new Error(ev))
        }
        const timer = setTimeout(bad, 8000)
        const cleanup = () => {
          clearTimeout(timer)
          v.removeEventListener(ev, ok)
          v.removeEventListener('error', bad)
        }
        v.addEventListener(ev, ok, { once: true })
        v.addEventListener('error', bad, { once: true })
      })
    ;(async () => {
      try {
        if (v.readyState < 1) await waitFor('loadedmetadata')
        for (let i = 0; i < times.length; i++) {
          if (cancelled || !ctx) return
          v.currentTime = times[i]
          await waitFor('seeked')
          if (cancelled) return
          const r = Math.min(canvas.width / (v.videoWidth || 16), canvas.height / (v.videoHeight || 9))
          const w = (v.videoWidth || 16) * r
          const h = (v.videoHeight || 9) * r
          ctx.fillStyle = '#000'
          ctx.fillRect(0, 0, canvas.width, canvas.height)
          ctx.drawImage(v, (canvas.width - w) / 2, (canvas.height - h) / 2, w, h)
          const data = canvas.toDataURL('image/jpeg', 0.72)
          setThumbs((prev) => prev.map((x, j) => (j === i ? data : x)))
        }
      } catch {
        if (!cancelled) setFailed(true)
      }
    })()
    return () => {
      cancelled = true
      v.removeAttribute('src')
      v.load()
    }
  }, [url, times])

  if (!duration) return <p className="pl-empty">{t('player.cenasAEsperar')}</p>
  const step = duration / Math.max(1, times.length)
  const active = Math.min(times.length - 1, Math.floor(now / step))
  return (
    <>
      {failed && <p className="pl-empty">{t('player.cenasErro')}</p>}
      <ol className="pl-frames__list">
        {times.map((s, i) => (
          <li key={i}>
            <button type="button" className={cx('pl-frame', i === active && 'is-active')} aria-current={i === active ? 'true' : undefined} aria-label={t('player.saltarPara', { tempo: formatDuration(s) })} onClick={() => onSeek(Math.max(0, s - step / 2))}>
              <span className="pl-frame__img">{thumbs[i] ? <img src={thumbs[i]!} alt="" /> : <span className="pl-frame__wait" />}</span>
              <span className="pl-frame__time dx-num">{formatDuration(Math.max(0, s - step / 2))}</span>
            </button>
          </li>
        ))}
      </ol>
    </>
  )
}
