/**
 * Quadro em tamanho real, num diálogo. O PNG vem autenticado (blob), por isso
 * o «Descarregar» é o próprio blob — não há outro URL que funcione sem Bearer.
 * O link público é o endpoint só-leitura do servidor
 * (`/api/public/whiteboards/{token}/image`), e só existe enquanto o quadro é público.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { publicWhiteboardImagePath, type WhiteboardMeta } from '../../api'
import { Icon } from '../../ui/icons'
import { Alert, Button, Dialog, IconButton, Spinner, TextInput } from '../../ui/kit'
import { formatDateTime } from '../recordings/format'
import { useBoardPng } from './useBoardPng'

export function boardShareUrl(token: string) {
  return location.origin + publicWhiteboardImagePath(token)
}

export default function BoardViewer({
  board,
  busy,
  onClose,
  onToggleShare,
  onOpenRoom,
  onDelete,
}: {
  board: WhiteboardMeta
  busy: boolean
  onClose: () => void
  onToggleShare: (b: WhiteboardMeta) => void
  onOpenRoom: (code: string) => void
  onDelete: (b: WhiteboardMeta) => void
}) {
  const { t, i18n } = useTranslation()
  const png = useBoardPng(board.id)
  const [copied, setCopied] = useState(false)
  const [copyErr, setCopyErr] = useState(false)
  const title = board.title || t('boards.semTitulo')
  const shareUrl = board.is_public && board.share_token ? boardShareUrl(board.share_token) : ''

  function copy() {
    setCopyErr(false)
    navigator.clipboard
      .writeText(shareUrl)
      .then(() => {
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      })
      .catch(() => setCopyErr(true))
  }

  return (
    <Dialog
      title={title}
      onClose={onClose}
      wide
      footer={
        <>
          <Button variant="ghost" icon="trash" onClick={() => onDelete(board)}>
            {t('ui.eliminar')}
          </Button>
          <span className="dx-spacer" />
          {board.room_code && (
            <Button variant="secondary" icon="door" onClick={() => onOpenRoom(board.room_code)}>
              {t('boards.visor.abrirSala')}
            </Button>
          )}
          {png.s === 'ready' && (
            <a className="dx-btn dx-btn--primary" href={png.url} download={[title, 'png'].join('.')}>
              <Icon name="download" />
              {t('boards.visor.descarregar')}
            </a>
          )}
        </>
      }
    >
      <p className="board-viewer__meta">
        <span className="dx-num">{formatDateTime(board.created_at, i18n.language)}</span>
        {board.room_code && (
          <>
            <span aria-hidden="true">·</span>
            <span>{t('boards.visor.sala', { code: board.room_code })}</span>
          </>
        )}
      </p>
      <div className="board-viewer__stage">
        {png.s === 'ready' ? (
          <img src={png.url} alt={title} />
        ) : png.s === 'error' ? (
          <Alert tone="danger">{t('boards.visor.erroImagem')}</Alert>
        ) : (
          <Spinner label={t('boards.visor.aCarregar')} />
        )}
      </div>
      <section className="board-viewer__share" aria-labelledby="board-share-title">
        <div className="board-viewer__share-head">
          <div>
            <h3 id="board-share-title">{t('boards.visor.link')}</h3>
            <p>{board.is_public ? t('boards.visor.linkActivoDica') : t('boards.visor.linkDica')}</p>
          </div>
          <Button size="sm" variant={board.is_public ? 'outline' : 'primary'} icon="link" busy={busy} onClick={() => onToggleShare(board)}>
            {board.is_public ? t('boards.accoes.desligarLink') : t('boards.accoes.criarLink')}
          </Button>
        </div>
        {shareUrl && (
          <div className="board-viewer__url">
            <TextInput code readOnly value={shareUrl} aria-label={t('boards.visor.link')} onFocus={(e) => e.target.select()} />
            <IconButton
              icon={copied ? 'check' : 'copy'}
              label={copied ? t('boards.visor.copiado') : t('boards.visor.copiar')}
              onClick={copy}
            />
          </div>
        )}
        {copyErr && <Alert tone="danger">{t('boards.visor.erroCopiar')}</Alert>}
      </section>
    </Dialog>
  )
}
