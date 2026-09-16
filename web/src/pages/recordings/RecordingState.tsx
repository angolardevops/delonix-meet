/**
 * Estado de uma gravação como o template o escreve: marca + texto na cor do
 * estado («✓ Pronta», «◐ A processar 74%», «⏱ Retida 90 dias»). As marcas são
 * ícones do kit, não caracteres.
 */
import { useTranslation } from 'react-i18next'
import { Icon, IconName } from '../../ui/icons'
import { cx } from '../../ui/kit'
import type { VisibleState } from './libraryData'

const ICON: Record<VisibleState['kind'], IconName> = {
  failed: 'alert',
  processing: 'hourglass',
  transcribing: 'hourglass',
  published: 'check',
  retained: 'clock',
  ready: 'check',
}

export function useStateText() {
  const { t } = useTranslation()
  return (s: VisibleState): string => {
    switch (s.kind) {
      case 'failed':
        return t('recordings.estado.falhada')
      case 'processing':
        return s.pct === null ? t('recordings.estado.aProcessar') : t('recordings.estado.aProcessarPct', { pct: Math.round(s.pct) })
      case 'transcribing':
        return s.pct === null ? t('recordings.estado.aTranscrever') : t('recordings.estado.aTranscreverPct', { pct: Math.round(s.pct) })
      case 'published':
        return t('recordings.estado.publicada')
      case 'retained':
        return t('recordings.estado.retida', { count: s.days })
      case 'ready':
        return t('recordings.estado.pronta')
    }
  }
}

export default function RecordingState({ state, lower }: { state: VisibleState; lower?: boolean }) {
  const text = useStateText()(state)
  return (
    <span className={cx('rec-state', `rec-state--${state.kind}`, lower && 'is-lower')}>
      <Icon name={ICON[state.kind]} size={11} />
      {text}
    </span>
  )
}
