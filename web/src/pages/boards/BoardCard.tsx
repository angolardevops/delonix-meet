/**
 * Cartão de um quadro guardado: pré-visualização (só descarregada quando o
 * cartão chega perto do ecrã), título, data e sala de origem, e as acções que
 * a API tem — link público, abrir a sala, eliminar.
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { WhiteboardMeta } from '../../api'
import { Icon } from '../../ui/icons'
import { Button, IconButton, Skeleton, StatusBadge } from '../../ui/kit'
import { formatDateTime } from '../recordings/format'
import { useBoardPng } from './useBoardPng'

export default function BoardCard({
  board,
  busy,
  onView,
  onToggleShare,
  onOpenRoom,
  onDelete,
}: {
  board: WhiteboardMeta
  busy: boolean
  onView: (b: WhiteboardMeta) => void
  onToggleShare: (b: WhiteboardMeta) => void
  onOpenRoom: (code: string) => void
  onDelete: (b: WhiteboardMeta) => void
}) {
  const { t, i18n } = useTranslation()
  const ref = useRef<HTMLLIElement>(null)
  const [near, setNear] = useState(typeof IntersectionObserver === 'undefined')
  useEffect(() => {
    const el = ref.current
    if (near || !el) return
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setNear(true)
          io.disconnect()
        }
      },
      { rootMargin: '200px' },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [near])
  const png = useBoardPng(board.id, near)
  const title = board.title || t('boards.semTitulo')

  return (
    <li ref={ref} className="board-card" data-public={board.is_public || undefined}>
      <button type="button" className="board-card__thumb" aria-label={t('boards.cartao.abrir', { title })} onClick={() => onView(board)}>
        {png.s === 'ready' ? (
          <img src={png.url} alt="" />
        ) : png.s === 'error' ? (
          <span className="board-card__nopreview">
            <Icon name="board" size={20} />
            <span>{t('boards.cartao.semPrevia')}</span>
          </span>
        ) : (
          <Skeleton h={120} />
        )}
      </button>
      <div className="board-card__body">
        <strong className="board-card__title" title={title}>
          {title}
        </strong>
        <span className="board-card__meta">
          <span className="dx-num">{formatDateTime(board.created_at, i18n.language)}</span>
          {board.room_code && (
            <>
              <span aria-hidden="true">·</span>
              <span className="dx-num">{board.room_code}</span>
            </>
          )}
        </span>
        <div className="board-card__foot">
          {board.is_public ? (
            <StatusBadge tone="success" icon="link">
              {t('boards.cartao.publico')}
            </StatusBadge>
          ) : (
            <StatusBadge tone="neutral">{t('boards.cartao.privado')}</StatusBadge>
          )}
          <span className="dx-spacer" />
          <Button
            size="sm"
            variant="ghost"
            icon="link"
            aria-pressed={board.is_public}
            busy={busy}
            onClick={() => onToggleShare(board)}
          >
            {board.is_public ? t('boards.accoes.desligarLink') : t('boards.accoes.criarLink')}
          </Button>
          {board.room_code && (
            <IconButton icon="door" bare label={t('boards.accoes.abrirSala', { code: board.room_code })} onClick={() => onOpenRoom(board.room_code)} />
          )}
          <IconButton icon="trash" bare label={t('boards.accoes.eliminar', { title })} onClick={() => onDelete(board)} />
        </div>
      </div>
    </li>
  )
}
