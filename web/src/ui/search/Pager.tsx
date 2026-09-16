/**
 * Paginador estilo Odoo: «1-50 / 234», o intervalo editável e ‹ ›.
 *
 * O servidor pagina por cursor (keyset, sem OFFSET): escrever «1-80» muda o
 * tamanho da página; escrever o intervalo de uma página já vista volta a ela.
 * Um salto para uma posição nunca vista é recusado e o intervalo repõe-se —
 * não se finge um OFFSET que o servidor não tem.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { IconButton } from '../kit'
import type { ListSearch } from './useSearch'

export default function Pager<T>({ list, compact }: { list: ListSearch<T>; compact?: boolean }) {
  const { t, i18n } = useTranslation()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState('')
  const [refused, setRefused] = useState(false)
  const d = list.state.s === 'ready' ? list.state.d : null
  if (!d || (d.total === 0 && !list.hasPrev)) return null
  const n = (x: number) => x.toLocaleString(i18n.language)
  const total = d.total_kind === 'at_least' ? t('search.paginador.pelomenos', { n: n(d.total) }) : n(d.total)
  const rangeText = list.range ? `${list.range.start}-${list.range.end}` : '0'

  function commit() {
    const ok = list.applyRange(text)
    setRefused(!ok && text.trim() !== rangeText)
    setEditing(false)
  }

  return (
    <nav className={`dx-pager${compact ? ' dx-pager--compact' : ''}`} aria-label={t('search.paginador.rotulo')}>
      {editing ? (
        <input
          className="dx-pager__input dx-num"
          autoFocus
          value={text}
          aria-label={t('search.paginador.editar')}
          title={t('search.paginador.ajuda')}
          size={Math.max(5, text.length + 1)}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              commit()
            } else if (e.key === 'Escape') {
              e.preventDefault()
              e.stopPropagation()
              setEditing(false)
            }
          }}
        />
      ) : (
        <button
          type="button"
          className="dx-pager__range dx-num"
          aria-label={t('search.paginador.editar')}
          title={refused ? t('search.paginador.recusado') : t('search.paginador.ajuda')}
          disabled={!list.range}
          onClick={() => {
            setText(rangeText)
            setEditing(true)
          }}
        >
          {rangeText}
        </button>
      )}
      <span className="dx-pager__total dx-num" aria-live="polite">
        / {total}
      </span>
      <IconButton icon="chevronLeft" bare label={t('search.paginador.anterior')} disabled={!list.hasPrev} onClick={list.prev} />
      <IconButton icon="chevronRight" bare label={t('search.paginador.seguinte')} disabled={!list.hasNext} onClick={list.next} />
      {refused && (
        <span className="dx-sr-only" role="status">
          {t('search.paginador.recusado')}
        </span>
      )}
    </nav>
  )
}
