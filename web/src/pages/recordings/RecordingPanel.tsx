/**
 * Painel direito da biblioteca (DelonixRecordings): leitor de 200 px com selos
 * e barra de progresso, título e metadados, acções, capítulos e transcrição.
 *
 * O vídeo NÃO se descarrega sozinho ao abrir a página: `recordingObjectUrl`
 * traz o ficheiro inteiro (o `<video>` não envia Bearer), e uma gravação de
 * uma hora são centenas de MB. Carrega quando a pessoa escolhe uma gravação
 * ou carrega em reproduzir; até lá mostra a miniatura do servidor, se houver.
 *
 * Só aparece o que tem dado ou acção real: Partilhar (dono), Descarregar
 * (quem pode), página inteira — é aí que se edita, publica, comenta e legenda.
 * «Exportar», «Guardar em…» e o cartão de armazenamento não têm servidor e
 * não se desenham. A secção de capítulos aparece quando os há.
 *
 * Este painel nunca recebe uma gravação falhada (R59) — a página não a deixa
 * seleccionar.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, downloadRecording } from '../../api'
import { useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Button, IconButton, Spinner, Tag } from '../../ui/kit'
import ChapterList from './ChapterList'
import { formatDateTime } from './format'
import { chapterAt, formatClock, resolutionLabel } from './libraryData'
import { clockPair, ProgressBar, usePlayback } from './playback'
import { useRecordingVideo } from './recordingMedia'
import { thumbStyle, useThumbnail } from './RecordingThumb'
import RecordingNotes from './RecordingNotes'
import { loadChapters, loadSegments } from './recordingLoaders'
import type { RecordingView } from './recordingView'
import { playerHash } from './studioLink'
import Transcript from './Transcript'

export default function RecordingPanel({
  rec,
  autoLoad,
  onShare,
  onClose,
}: {
  rec: RecordingView
  autoLoad: boolean
  onShare: (r: RecordingView) => void
  onClose: () => void
}) {
  const { t, i18n } = useTranslation()
  const videoRef = useRef<HTMLVideoElement>(null)
  const [want, setWant] = useState(autoLoad)
  const [video, retry] = useRecordingVideo(rec, want)
  const src = video.s === 'ready' ? video.url : null
  const pb = usePlayback(videoRef, src)
  const [actionErr, setActionErr] = useState('')
  const [downloading, setDownloading] = useState(false)
  const thumb = useThumbnail(rec)
  const extra = useAsync(async (signal) => {
    const [chapters, segments] = await Promise.all([loadChapters(rec, signal), loadSegments(rec, signal)])
    return { chapters, segments }
  }, [rec.id])
  const chapters = extra.state.s === 'ready' ? extra.state.d.chapters : null
  const segments = extra.state.s === 'ready' ? extra.state.d.segments : null

  // Escolher de novo a gravação que já estava seleccionada por omissão conta
  // como pedido para a ver.
  useEffect(() => {
    if (autoLoad) setWant(true)
  }, [autoLoad])

  const durationMs = rec.durationMs ?? pb.durationMs
  const res = resolutionLabel(rec.height !== null ? rec : pb.size)

  function seek(ms: number) {
    if (src) pb.seek(ms, true)
    else {
      pb.queueSeek(ms)
      setWant(true)
    }
  }

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

  const meta = [
    formatDateTime(rec.createdAt, i18n.language),
    rec.uploaderName,
    t('recordings.salaCodigo', { code: rec.roomCode }),
    rec.participantCount !== null ? t('recordings.leitor.participantes', { count: rec.participantCount }) : null,
  ].filter(Boolean)

  return (
    <div className="rec-panel__inner">
      <header className="rec-panel__head">
        <span className="dx-eyebrow">{t('recordings.leitor.rotulo')}</span>
        <span className="dx-spacer" />
        <IconButton icon="x" bare label={t('recordings.leitor.fechar')} className="rec-panel__close" onClick={onClose} />
      </header>

      <div className="rec-player">
        {src ? (
          <video ref={videoRef} className="rec-player__video" src={src} autoPlay playsInline onClick={pb.toggle} />
        ) : (
          <div className="rec-player__poster" style={thumbStyle(thumb, rec.name)} />
        )}
        {video.s === 'loading' ? (
          <div className="rec-player__center">
            <Spinner label={t('recordings.leitor.aCarregar')} />
          </div>
        ) : video.s === 'error' ? (
          <div className="rec-player__center rec-player__error" role="alert">
            <Icon name="alert" />
            <span>{t('recordings.leitor.erroVideo')}</span>
            <Button size="sm" variant="secondary" icon="refresh" onClick={retry}>
              {t('ui.tentarDeNovo')}
            </Button>
          </div>
        ) : (
          !pb.playing && (
            <div className="rec-player__center">
              <button
                type="button"
                className="rec-player__play"
                aria-label={t('recordings.leitor.reproduzir', { name: rec.name })}
                onClick={() => (src ? pb.toggle() : setWant(true))}
              >
                <Icon name="play" size={18} />
              </button>
            </div>
          )
        )}
        {(res || rec.category) && (
          <div className="rec-player__badges">
            {res && (
              <span className="rec-badge" title={t('recordings.leitor.resolucao')}>
                {res}
              </span>
            )}
            {rec.category && <span className="rec-badge is-soft">{t(`recordings.categoria.${rec.category}`)}</span>}
          </div>
        )}
        <div className="rec-player__foot">
          <ProgressBar
            nowMs={pb.nowMs}
            durationMs={durationMs}
            ticksMs={chapters?.map((c) => c.tMs) ?? []}
            onSeek={seek}
            label={t('recordings.leitor.progresso')}
            valueText={clockPair(pb.nowMs, durationMs)}
          />
          <div className="rec-player__times dx-num">
            <span>{formatClock(pb.nowMs)}</span>
            <span title={t('recordings.leitor.duracao')}>{formatClock(durationMs)}</span>
          </div>
        </div>
      </div>

      <div className="rec-panel__title">
        <h2>{rec.name}</h2>
        <p className="rec-panel__meta">
          {meta.join(' · ')}
          {!rec.owned && <Tag plain>{t('recordings.partilhadaComigo')}</Tag>}
        </p>
      </div>

      <div className="rec-panel__actions">
        {rec.owned && (
          <Button variant="primary" size="sm" onClick={() => onShare(rec)}>
            {rec.shareCount > 0 ? t('recordings.accoes.partilharN', { count: rec.shareCount }) : t('recordings.accoes.partilhar')}
          </Button>
        )}
        {rec.canDownload && (
          <Button size="sm" variant={rec.owned ? 'secondary' : 'primary'} busy={downloading} onClick={() => void download()}>
            {t('recordings.accoes.descarregar')}
          </Button>
        )}
        <Button size="sm" variant="secondary" onClick={() => (location.hash = playerHash(rec.id).slice(1))}>
          {t('player.paginaInteira')}
        </Button>
      </div>
      {actionErr && <Alert tone="danger">{actionErr}</Alert>}

      {chapters && chapters.length > 0 && (
        <section className="rec-section" aria-labelledby="rec-chapters-title">
          <div className="rec-section__head">
            <h3 id="rec-chapters-title">{chapters.every((c) => c.auto) ? t('recordings.capitulos.automaticos') : t('recordings.capitulos.titulo')}</h3>
            <span className="rec-section__aside dx-num">{t('recordings.capitulos.detectados', { count: chapters.length })}</span>
          </div>
          <ChapterList chapters={chapters} active={chapterAt(chapters, pb.nowMs)} onSeek={seek} variant="rows" />
        </section>
      )}

      {/* Enquanto os sub-recursos carregam não se pedem as notas da sala: com
          transcrição do servidor elas nem se mostram. */}
      {extra.state.s === 'loading' ? null : segments ? (
        <section className="rec-section" aria-labelledby="rec-transcript-title">
          <div className="rec-section__head">
            <h3 id="rec-transcript-title">{t('recordings.notas.transcricao')}</h3>
            {rec.transcriptLanguage && (
              <span className="rec-section__aside is-ok dx-num">
                <Icon name="check" size={10} /> {t('recordings.transcricao.gerada', { lang: rec.transcriptLanguage })}
              </span>
            )}
          </div>
          <Transcript segments={segments} nowMs={pb.nowMs} onSeek={seek} />
        </section>
      ) : (
        <RecordingNotes roomCode={rec.roomCode} compact />
      )}
    </div>
  )
}
