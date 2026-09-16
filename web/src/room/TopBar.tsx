import { useTranslation } from 'react-i18next'
import type { CallState } from '../callRecovery'
import { DelonixSymbol, Icon } from '../ui/icons'
import { Button, Segmented, StatusBadge, Tag, cx } from '../ui/kit'
import { MeetingElapsed, WallClock } from './Clocks'
import type { ViewMode } from './useLayout'

/** Estado da ligação de media em palavras e com forma, nunca só cor. */
function ConnectionChip({ state }: { state: CallState }) {
  const { t } = useTranslation()
  if (state === 'connected')
    return (
      <span className="rm-conn rm-conn--ok">
        <Icon name="check" size={11} />
        {t('room.topo.ligacaoEstavel')}
      </span>
    )
  if (state === 'degraded')
    return (
      <span className="rm-conn rm-conn--warn">
        <Icon name="alert" size={11} />
        {t('room.topo.ligacaoInstavel')}
      </span>
    )
  if (state === 'reconnecting' || state === 'recovering' || state === 'failed')
    return (
      <span className="rm-conn rm-conn--warn">
        <Icon name="refresh" size={11} />
        {t('room.topo.aRestabelecer')}
      </span>
    )
  return (
    <span className="rm-conn">
      <Icon name="wifi" size={11} />
      {t('room.topo.aLigar')}
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
  isInstant,
  isTraining,
  waitingCount,
  onOpenPeople,
  total,
  viewMode,
  onViewMode,
  studioAvailable,
  studioOpen,
  onStudio,
  board,
  locale,
}: {
  title: string
  code: string
  joinedAt: number
  inRoom: boolean
  callState: CallState
  /** Texto do indicador de gravação, ou `null` se ninguém grava. */
  recordingLabel: string | null
  live: boolean
  e2eeOn: boolean
  secOpen: boolean
  secCode: string
  onToggleSec: () => void
  isInstant: boolean
  isTraining: boolean
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
  board: { sharedBy: string | null; saving: boolean; canSave: boolean; onSave: () => void; onClose: () => void; pen: { on: boolean; pressao: boolean } } | null
  locale: string
}) {
  const { t } = useTranslation()
  return (
    <header className="rm-top">
      <span className="rm-top__mark" aria-hidden="true">
        <DelonixSymbol size={18} />
      </span>
      <h1 className="rm-top__title">
        {board
          ? board.sharedBy
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
      {live && <StatusBadge tone="live">{t('room.topo.aoVivo')}</StatusBadge>}
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
      <span className={cx('rm-hide-narrow rm-top__tags', board && 'is-hidden')}>
        {isInstant && <Tag>{t('room.topo.instantanea')}</Tag>}
        {isTraining && <Tag>{t('room.topo.formacao')}</Tag>}
        <span className="rm-top__meta dx-num">{code}</span>
      </span>
      <span className="dx-spacer" />
      {waitingCount > 0 && (
        <button type="button" className="rm-waiting-pill" onClick={onOpenPeople}>
          <Icon name="people" size={12} />
          {t('room.topo.aEspera', { count: waitingCount })}
        </button>
      )}
      {board && (
        <>
          {board.sharedBy && <span className="rm-top__meta rm-hide-narrow">{t('room.quadro.abertoPor', { nome: board.sharedBy })}</span>}
          {board.pen.on && (
            <span className="rm-top__pen rm-hide-narrow">
              <span className="rm-top__penchip">
                <Icon name="pen" size={11} />
                {t('room.quadro.caneta')}
              </span>
              {board.pen.pressao && <span className="rm-top__meta dx-num">{t('room.quadro.pressaoActiva')}</span>}
            </span>
          )}
          <Button size="sm" variant="outline" icon="download" busy={board.saving} disabled={!board.canSave} onClick={board.onSave}>
            <span className="rm-hide-narrow">{t('room.quadro.guardar')}</span>
          </Button>
          <Button size="sm" variant="primary" icon="x" onClick={board.onClose}>
            <span className="rm-hide-narrow">{t('room.quadro.fechar')}</span>
          </Button>
        </>
      )}
      <span className={cx('rm-hide-narrow', 'rm-top__conn', board && 'is-hidden')}>
        <ConnectionChip state={callState} />
        <WallClock locale={locale} className="dx-num" />
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
      <button type="button" className="rm-top__count" onClick={onOpenPeople} aria-label={t('room.topo.participantes', { count: total })}>
        <Icon name="people" size={13} />
        <span className="dx-num">{total}</span>
      </button>
    </header>
  )
}
