import { useTranslation } from 'react-i18next'
import type { CallState } from '../callRecovery'
import { DelonixSymbol, Icon } from '../ui/icons'
import { Segmented, StatusBadge, Tag, cx } from '../ui/kit'
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
  locale: string
}) {
  const { t } = useTranslation()
  return (
    <header className="rm-top">
      <span className="rm-top__mark" aria-hidden="true">
        <DelonixSymbol size={18} />
      </span>
      <h1 className="rm-top__title">{title || code}</h1>
      {inRoom && <MeetingElapsed startedAt={joinedAt} className="rm-top__elapsed dx-num" />}
      {recordingLabel && (
        <span title={recordingLabel}>
          <StatusBadge tone="record">{t('room.topo.rec')}</StatusBadge>
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
      <span className="rm-hide-narrow rm-top__tags">
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
      <span className={cx('rm-hide-narrow', 'rm-top__conn')}>
        <ConnectionChip state={callState} />
        <WallClock locale={locale} className="dx-num" />
      </span>
      <span className="rm-hide-narrow">
        <Segmented<ViewMode>
          label={t('room.topo.vista')}
          value={viewMode}
          onChange={onViewMode}
          options={[
            { value: 'grid', label: t('room.topo.grelha') },
            { value: 'stage', label: t('room.topo.orador') },
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
