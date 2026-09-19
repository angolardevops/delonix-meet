import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { CallState } from '../callRecovery'
import { DelonixSymbol, Icon } from '../ui/icons'
import { AvatarStack, Button, Segmented, StatusBadge, cx } from '../ui/kit'
import { MeetingElapsed, WallClock } from './Clocks'
import type { ViewMode } from './useLayout'

/** Estado da ligação de media em palavras e com forma, nunca só cor. */
function ConnectionChip({ state, children }: { state: CallState; children?: ReactNode }) {
  const { t } = useTranslation()
  const [classe, icone, texto] =
    state === 'connected'
      ? ['rm-conn--ok', 'check', t('room.topo.ligacaoEstavel')]
      : state === 'degraded'
        ? ['rm-conn--warn', 'alert', t('room.topo.ligacaoInstavel')]
        : state === 'reconnecting' || state === 'recovering' || state === 'failed'
          ? ['rm-conn--warn', 'refresh', t('room.topo.aRestabelecer')]
          : ['', 'wifi', t('room.topo.aLigar')]
  // Estado e hora no mesmo chip, como no template: «✓ Ligação estável · WAT 10:24».
  return (
    <span className={cx('rm-conn', classe)}>
      <Icon name={icone} size={11} className="dx-icon rm-conn__icon" />
      <span className="rm-conn__text">{texto}</span>
      {children && <span className="rm-conn__clock"> · {children}</span>}
    </span>
  )
}

export function TopBar({
  title,
  code,
  joinedAt,
  inRoom,
  callState,
  recordingLabel,
  live,
  e2eeOn,
  secOpen,
  secCode,
  onToggleSec,
  waitingCount,
  onOpenPeople,
  total,
  viewMode,
  onViewMode,
  studioAvailable,
  studioOpen,
  onStudio,
  board,
  presenterLabel,
  locale,
}: {
  title: string
  code: string
  joinedAt: number
  inRoom: boolean
  callState: CallState
  /** Texto do indicador de gravação, ou `null` se ninguém grava. */
  recordingLabel: string | null
  /** Estado AO VIVO da sala (anunciado pelo servidor), ou emissão local deste anfitrião. */
  live: { on: boolean; destinos: { label: string; state: string }[]; since: number | null }
  e2eeOn: boolean
  secOpen: boolean
  secCode: string
  onToggleSec: () => void
  waitingCount: number
  onOpenPeople: () => void
  total: number
  viewMode: ViewMode
  onViewMode: (v: ViewMode) => void
  /** Anfitrião com multicâmara disponível: o terceiro modo abre o estúdio da sala. */
  studioAvailable: boolean
  studioOpen: boolean
  onStudio: () => void
  /** Quadro aberto: a barra passa a ser a do quadro (template DelonixWhiteboard). */
  /** «A partilhar · Nome» quando há apresentação (template DelonixRoomChat). */
  presenterLabel: string | null
  board: {
    sharedBy: string | null
    saving: boolean
    canSave: boolean
    onSave: () => void
    pen: { on: boolean; pressao: boolean }
    /** Quem mexeu no quadro nos últimos segundos («A editar:»). */
    editors: string[]
    /** Quadro partilhado (caneta, ajustes, palco): a barra é a do template DelonixBoardShared. */
    shared: boolean
  } | null
  locale: string
}) {
  const { t } = useTranslation()
  return (
    <header className={cx('rm-top', board && 'is-board')}>
      <span className="rm-top__mark" aria-hidden="true">
        <DelonixSymbol size={18} />
      </span>
      <h1 className="rm-top__title">
        {board
          ? board.shared
            ? t('room.quadro.tituloPartilhado', { nome: title || code })
            : t('room.quadro.tituloBarra', { nome: title || code })
          : title || code}
      </h1>
      {inRoom && <MeetingElapsed startedAt={joinedAt} className="rm-top__elapsed dx-num" />}
      {recordingLabel && (
        <span title={recordingLabel} role="status">
          <StatusBadge tone="record">{t('room.topo.rec')}</StatusBadge>
          <span className="dx-sr-only">{recordingLabel}</span>
        </span>
      )}
      {live.on && (
        <span
          role="status"
          title={
            live.destinos.length
              ? t('room.topo.aoVivoDica', {
                  hora: live.since ? new Date(live.since).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' }) : '—',
                  destinos: live.destinos.map((d) => d.label).join(', '),
                })
              : undefined
          }
        >
          <StatusBadge tone="live">
            {live.destinos.length ? t('room.topo.aoVivoDestinos', { count: live.destinos.length }) : t('room.topo.aoVivo')}
          </StatusBadge>
        </span>
      )}
      {e2eeOn && (
        <button type="button" className="rm-top__e2ee" onClick={onToggleSec} aria-expanded={secOpen} title={t('room.topo.e2eeDica')}>
          <Icon name="lock" size={11} />
          {t('room.topo.e2ee')}
        </button>
      )}
      {secOpen && secCode && (
        <span className="rm-top__sec dx-num" title={t('room.topo.codigoSegurancaDica')}>
          <Icon name="shieldCheck" size={12} />
          {secCode}
        </span>
      )}
      <span className="dx-spacer" />
      {/* No desktop a fila de espera vive na barra de baixo (template); no telemóvel, aqui. */}
      {waitingCount > 0 && (
        <button type="button" className="rm-waiting-pill rm-only-narrow-inline" onClick={onOpenPeople}>
          <Icon name="people" size={12} />
          {t('room.topo.aEspera', { count: waitingCount })}
        </button>
      )}
      {board && (
        <>
          {board.shared ? (
            board.pen.on && (
              <span className="rm-top__pen rm-hide-narrow">
                <span className="rm-top__penchip">
                  <Icon name="pen" size={11} />
                  {t('room.quadro.caneta')}
                </span>
                {board.pen.pressao && <span className="rm-top__meta dx-num">{t('room.quadro.pressaoActiva')}</span>}
              </span>
            )
          ) : (
            <>
              {board.editors.length > 0 && (
                <span className="rm-top__editing rm-hide-narrow" role="status" aria-label={t('room.quadro.aEditarNomes', { nomes: board.editors.join(', ') })}>
                  <span aria-hidden="true">{t('room.quadro.aEditar')}</span>
                  <AvatarStack names={board.editors} max={4} size={22} />
                </span>
              )}
              <Button size="sm" variant="outline" busy={board.saving} disabled={!board.canSave} onClick={board.onSave} className="rm-top__save">
                <span className="rm-hide-narrow">{t('room.quadro.guardar')}</span>
              </Button>
            </>
          )}
        </>
      )}
      {presenterLabel && !board ? <span className="rm-top__presenting rm-hide-narrow">{presenterLabel}</span> : null}
      <span className={cx('rm-hide-narrow', 'rm-top__conn', (board || presenterLabel) && 'is-hidden')}>
        <ConnectionChip state={callState}>
          <WallClock locale={locale} />
        </ConnectionChip>
      </span>
      <span className={cx('rm-hide-narrow', board && 'is-hidden')}>
        <Segmented<ViewMode | 'studio'>
          label={t('room.topo.vista')}
          value={studioOpen ? 'studio' : viewMode}
          onChange={(v) => (v === 'studio' ? onStudio() : onViewMode(v))}
          options={[
            { value: 'grid', label: t('room.topo.grelha') },
            { value: 'stage', label: t('room.topo.orador') },
            ...(studioAvailable ? [{ value: 'studio' as const, label: t('room.topo.estudio') }] : []),
          ]}
        />
      </span>
      {/* No desktop a ocupação vive na barra de baixo; no telemóvel, aqui. */}
      <div className="rm-only-narrow">
        <button type="button" className="rm-top__count" onClick={onOpenPeople} aria-label={t('room.topo.participantes', { count: total })}>
          <Icon name="people" size={13} />
          <span className="dx-num">{total}</span>
        </button>
      </div>
    </header>
  )
}
