/**
 * Painel direito do template: leitor, título, acções e notas.
 *
 * O vídeo NÃO se descarrega sozinho ao abrir a página: `recordingObjectUrl`
 * traz o ficheiro inteiro (o `<video>` não envia Bearer), e uma gravação de
 * uma hora são centenas de MB. Carrega quando a pessoa escolhe uma gravação
 * ou carrega em reproduzir.
 *
 * Os controlos são os do próprio `<video controls>`. Do template ficam os que
 * têm dado real: duração e resolução lidas do ficheiro (não da API, que não
 * as tem), janela flutuante onde o browser a suporta. Capítulos, comentários
 * com marca temporal, «Publicar», «Exportar» e o cartão de armazenamento não
 * existem no servidor e não aparecem.
 *
 * Este painel nunca recebe uma gravação falhada (R59) — a página não a deixa
 * seleccionar. A guarda de baixo é a segunda linha, não a primeira.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, downloadRecording, RecordingItem, recordingObjectUrl } from '../../api'
import { Icon } from '../../ui/icons'
import { Alert, Button, IconButton, Spinner, Tag } from '../../ui/kit'
import { formatBytes, formatDateTime, formatDuration, isFailed, recordingName, thumbBackground } from './format'
import RecordingNotes from './RecordingNotes'

type Video = { s: 'idle' } | { s: 'loading' } | { s: 'ready'; url: string } | { s: 'error' }

export default function RecordingPanel({
  rec,
  autoLoad,
  onShare,
  onClose,
}: {
  rec: RecordingItem
  autoLoad: boolean
  onShare: (r: RecordingItem) => void
  onClose: () => void
}) {
  const { t, i18n } = useTranslation()
  const videoRef = useRef<HTMLVideoElement>(null)
  const [video, setVideo] = useState<Video>({ s: 'idle' })
  const [want, setWant] = useState(autoLoad)
  const [attempt, setAttempt] = useState(0)
  const [duration, setDuration] = useState<number | null>(null)
  const [size, setSize] = useState<{ w: number; h: number } | null>(null)
  const [actionErr, setActionErr] = useState('')
  const [downloading, setDownloading] = useState(false)
  const pipAvailable = typeof document !== 'undefined' && document.pictureInPictureEnabled === true
  const failed = isFailed(rec)
  const name = recordingName(rec)

  // Escolher de novo a gravação que já estava seleccionada por omissão conta
  // como pedido para a ver.
  useEffect(() => {
    if (autoLoad) setWant(true)
  }, [autoLoad])

  useEffect(() => {
    if (!want || failed) return
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
      .catch(() => {
        if (live) setVideo({ s: 'error' })
      })
    return () => {
      live = false
      if (made) URL.revokeObjectURL(made)
    }
  }, [rec, want, failed, attempt])

  function onMetadata() {
    const v = videoRef.current
    if (!v) return
    if (v.videoWidth && v.videoHeight) setSize({ w: v.videoWidth, h: v.videoHeight })
    if (Number.isFinite(v.duration)) {
      setDuration(v.duration)
    } else {
      // WebM do MediaRecorder chega sem duração no cabeçalho: saltar para o
      // fim obriga o browser a medi-la, e volta-se ao início logo a seguir.
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

  async function pip() {
    const v = videoRef.current
    if (!v) return
    setActionErr('')
    try {
      await v.requestPictureInPicture()
    } catch {
      setActionErr(t('recordings.leitor.pipRecusada'))
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

  return (
    <div className="rec-panel__inner">
      <header className="rec-panel__head">
        <span className="dx-eyebrow">{t('recordings.leitor.rotulo')}</span>
        <span className="dx-spacer" />
        <IconButton icon="x" bare label={t('recordings.leitor.fechar')} className="rec-panel__close" onClick={onClose} />
      </header>

      {failed ? (
        <Alert tone="danger" icon="alert">
          {rec.failure_reason || t('recordings.estado.semCausa')}
        </Alert>
      ) : (
        <div className="rec-player">
          {video.s === 'ready' ? (
            <video
              ref={videoRef}
              className="rec-player__video"
              src={video.url}
              controls
              autoPlay
              playsInline
              onLoadedMetadata={onMetadata}
            />
          ) : (
            <div className="rec-player__poster" style={{ background: thumbBackground(rec.filename) }}>
              {video.s === 'loading' ? (
                <Spinner label={t('recordings.leitor.aCarregar')} />
              ) : video.s === 'error' ? (
                <div className="rec-player__error" role="alert">
                  <Icon name="alert" />
                  <span>{t('recordings.leitor.erroVideo')}</span>
                  <Button size="sm" variant="secondary" icon="refresh" onClick={() => setAttempt((n) => n + 1)}>
                    {t('ui.tentarDeNovo')}
                  </Button>
                </div>
              ) : (
                <button
                  type="button"
                  className="rec-player__play"
                  aria-label={t('recordings.leitor.reproduzir', { name })}
                  onClick={() => setWant(true)}
                >
                  <Icon name="play" size={20} />
                </button>
              )}
            </div>
          )}
          {(size || duration !== null) && (
            <div className="rec-player__badges">
              {size && (
                <span className="rec-player__badge dx-num" title={t('recordings.leitor.resolucao')}>
                  {size.w}×{size.h}
                </span>
              )}
              {duration !== null && (
                <span className="rec-player__badge dx-num" title={t('recordings.leitor.duracao')}>
                  {formatDuration(duration)}
                </span>
              )}
            </div>
          )}
        </div>
      )}

      <div className="rec-panel__title">
        <h2>{name}</h2>
        <p className="rec-panel__meta">
          <span className="dx-num">{formatDateTime(rec.created_at, i18n.language)}</span>
          <span aria-hidden="true">·</span>
          <span>{rec.uploader_name}</span>
          <span aria-hidden="true">·</span>
          <span>{t('recordings.salaCodigo', { code: rec.room_code })}</span>
          {!failed && (
            <>
              <span aria-hidden="true">·</span>
              <span className="dx-num">{formatBytes(rec.size_bytes, i18n.language)}</span>
            </>
          )}
          {!rec.owned && <Tag plain>{t('recordings.partilhadaComigo')}</Tag>}
        </p>
      </div>

      {!failed && (
        <div className="rec-panel__actions">
          {rec.owned && (
            <Button variant="primary" size="sm" icon="share" onClick={() => onShare(rec)}>
              {rec.share_count > 0
                ? t('recordings.accoes.partilharN', { count: rec.share_count })
                : t('recordings.accoes.partilhar')}
            </Button>
          )}
          {rec.can_download && (
            <Button size="sm" icon="download" busy={downloading} onClick={() => void download()}>
              {t('recordings.accoes.descarregar')}
            </Button>
          )}
          {pipAvailable && video.s === 'ready' && (
            <Button size="sm" variant="ghost" icon="pip" onClick={() => void pip()}>
              {t('recordings.leitor.pip')}
            </Button>
          )}
        </div>
      )}
      {actionErr && <Alert tone="danger">{actionErr}</Alert>}

      <RecordingNotes roomCode={rec.room_code} />
    </div>
  )
}
