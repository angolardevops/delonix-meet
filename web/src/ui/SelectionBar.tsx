/**
 * Barra flutuante «N seleccionados» do template v5 (DelonixWhiteboard,
 * DelonixBoardShared, DelonixCanvasUML e DelonixCanvasBPMN): Agrupar ⌘G,
 * Desagrupar ⇧⌘G, alinhar/distribuir e apagar. Não sabe nada do editor —
 * recebe o que pode fazer e chama de volta.
 */
import type { CSSProperties } from 'react'
import { useTranslation } from 'react-i18next'
import { isApplePlatform, type AlignMode, type Axis } from './arrange'
import { Icon, type IconName } from './icons'
import './selection.css'

/** Os rótulos dos atalhos na plataforma de quem está a ver (⌘ na Apple, Ctrl nos outros). */
export function useGroupShortcutLabels() {
  const { t } = useTranslation()
  const mac = isApplePlatform()
  return {
    group: t(mac ? 'ui.seleccao.teclaAgruparMac' : 'ui.seleccao.teclaAgrupar'),
    ungroup: t(mac ? 'ui.seleccao.teclaDesagruparMac' : 'ui.seleccao.teclaDesagrupar'),
  }
}

/** Valores de `aria-keyshortcuts` (sintaxe da norma ARIA, não texto de interface). */
export const ARIA_KEYS = { group: ['Meta+G', 'Control+G'].join(' '), ungroup: ['Shift+Meta+G', 'Shift+Control+G'].join(' ') }

const ALIGN: { mode: AlignMode; icon: IconName; label: string }[] = [
  { mode: 'left', icon: 'selAlignLeft', label: 'ui.seleccao.alinharEsquerda' },
  { mode: 'centerX', icon: 'selAlignCenter', label: 'ui.seleccao.alinharCentro' },
  { mode: 'top', icon: 'selAlignTop', label: 'ui.seleccao.alinharTopo' },
]

export function SelectionBar({
  count,
  canGroup,
  canUngroup,
  canDelete = true,
  onGroup,
  onUngroup,
  onAlign,
  onDistribute,
  onDelete,
  style,
  className,
}: {
  count: number
  canGroup: boolean
  canUngroup: boolean
  canDelete?: boolean
  onGroup: () => void
  onUngroup: () => void
  onAlign: (mode: AlignMode) => void
  onDistribute: (axis: Axis) => void
  onDelete: () => void
  style?: CSSProperties
  className?: string
}) {
  const { t } = useTranslation()
  const keys = useGroupShortcutLabels()
  // Alinhar precisa de duas unidades; distribuir, de três. Quem chama conta as unidades.
  const stop = (e: { stopPropagation: () => void }) => e.stopPropagation()
  return (
    <div
      className={['dx-selbar', className].filter(Boolean).join(' ')}
      role="toolbar"
      aria-label={t('ui.seleccao.barra')}
      style={style}
      data-selbar
      onPointerDown={stop}
    >
      <span className="dx-selbar__count" aria-live="polite">
        {t('ui.seleccao.seleccionados', { count })}
      </span>
      <span className="dx-selbar__sep" aria-hidden="true" />
      <button type="button" className="dx-selbar__btn is-primary" disabled={!canGroup} onClick={onGroup} aria-keyshortcuts={ARIA_KEYS.group}>
        <span>{t('ui.seleccao.agrupar')}</span>
        <kbd>{keys.group}</kbd>
      </button>
      <button type="button" className="dx-selbar__btn" disabled={!canUngroup} onClick={onUngroup} aria-keyshortcuts={ARIA_KEYS.ungroup}>
        <span>{t('ui.seleccao.desagrupar')}</span>
        <kbd>{keys.ungroup}</kbd>
      </button>
      <span className="dx-selbar__sep" aria-hidden="true" />
      <span className="dx-selbar__icons">
        {ALIGN.map((a) => (
          <button key={a.mode} type="button" className="dx-selbar__icon" data-align={a.mode} aria-label={t(a.label)} title={t(a.label)} disabled={count < 2} onClick={() => onAlign(a.mode)}>
            <Icon name={a.icon} size={13} strokeWidth={1.8} />
          </button>
        ))}
        <button type="button" className="dx-selbar__icon" data-distribute="x" aria-label={t('ui.seleccao.distribuirH')} title={t('ui.seleccao.distribuirH')} disabled={count < 3} onClick={() => onDistribute('x')}>
          <Icon name="selDistributeH" size={13} strokeWidth={1.7} />
        </button>
      </span>
      <span className="dx-selbar__sep" aria-hidden="true" />
      <button type="button" className="dx-selbar__icon is-danger" aria-label={t('ui.seleccao.apagar')} title={t('ui.seleccao.apagar')} disabled={!canDelete} onClick={onDelete}>
        <Icon name="selTrash" size={13} strokeWidth={1.7} />
      </button>
    </div>
  )
}
