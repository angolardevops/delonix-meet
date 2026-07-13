import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  authedBlobUrl,
  deleteWhiteboard,
  listWhiteboards,
  shareWhiteboard,
  whiteboardPngUrl,
  WhiteboardMeta,
} from '../api'
import { CloseIcon, DownloadIcon, ShareLinkIcon, TrashIcon } from '../icons'

/** Miniatura autenticada de um quadro (o endpoint PNG exige Bearer). */
function Thumb({ id, title }: { id: string; title: string }) {
  const [url, setUrl] = useState<string>()
  useEffect(() => {
    let live = true
    let made: string | undefined
    authedBlobUrl(whiteboardPngUrl(id))
      .then((u) => {
        if (live) { made = u; setUrl(u) }
        else URL.revokeObjectURL(u)
      })
      .catch(() => {})
    return () => { live = false; if (made) URL.revokeObjectURL(made) }
  }, [id])
  return url ? <img className="wb-thumb" src={url} alt={title} /> : <div className="wb-thumb ph" />
}

/** Quadro em tamanho real dentro do lightbox (fetch autenticado → blob). */
function FullBoard({ id, title, loadingLabel, downloadLabel }: {
  id: string
  title: string
  loadingLabel: string
  downloadLabel: string
}) {
  const [url, setUrl] = useState<string>()
  useEffect(() => {
    let live = true
    let made: string | undefined
    authedBlobUrl(whiteboardPngUrl(id))
      .then((u) => {
        if (live) { made = u; setUrl(u) }
        else URL.revokeObjectURL(u)
      })
      .catch(() => {})
    return () => { live = false; if (made) URL.revokeObjectURL(made) }
  }, [id])
  if (!url) return <p className="muted">{loadingLabel}</p>
  return (
    <>
      <img className="wb-full" src={url} alt={title} />
      <a className="btn-sm ghost wb-download" href={url} download={`${title}.png`}>
        <DownloadIcon /> {downloadLabel}
      </a>
    </>
  )
}

/** Biblioteca de quadros brancos guardados (por organização). */
export default function Whiteboards() {
  const { t } = useTranslation()
  const [items, setItems] = useState<WhiteboardMeta[]>([])
  const [loading, setLoading] = useState(true)
  // Lightbox: o endpoint PNG exige Bearer — abrir o URL cru num separador
  // novo dá "unauthorized"; o quadro abre aqui via blob autenticado.
  const [view, setView] = useState<WhiteboardMeta | null>(null)

  const refresh = () =>
    listWhiteboards().then(setItems).catch(() => setItems([])).finally(() => setLoading(false))
  useEffect(() => { void refresh() }, [])

  async function toggleShare(w: WhiteboardMeta) {
    const updated = await shareWhiteboard(w.id, !w.is_public).catch(() => null)
    if (updated) {
      setItems((xs) => xs.map((x) => (x.id === w.id ? updated : x)))
      if (updated.is_public) {
        const link = `${location.origin}/api/whiteboards/shared/${updated.share_token}`
        void navigator.clipboard.writeText(link).catch(() => {})
      }
    }
  }

  async function remove(w: WhiteboardMeta) {
    if (!confirm(t('wb.confirmDelete', { title: w.title }))) return
    await deleteWhiteboard(w.id).catch(() => {})
    refresh()
  }

  return (
    <div className="page wb-page">
      <header className="page-head">
        <h1>{t('wb.title')}</h1>
        <p className="muted">{t('wb.sub')}</p>
      </header>

      {loading ? (
        <p className="dash-empty">{t('common.loading')}</p>
      ) : items.length === 0 ? (
        <p className="dash-empty">{t('wb.empty')}</p>
      ) : (
        <div className="wb-grid">
          {items.map((w) => (
            <article key={w.id} className="wb-card">
              <button className="wb-thumb-link" onClick={() => setView(w)} title={w.title}>
                <Thumb id={w.id} title={w.title} />
              </button>
              <div className="wb-card-body">
                <h3 title={w.title}>{w.title}</h3>
                <p className="wb-meta">
                  {new Date(w.created_at).toLocaleString()}
                  {w.room_code && <> · <span className="mono">{w.room_code}</span></>}
                </p>
                <div className="wb-actions">
                  <button
                    className={w.is_public ? 'chip-btn on' : 'chip-btn'}
                    onClick={() => void toggleShare(w)}
                    title={w.is_public ? t('wb.sharedOn') : t('wb.share')}
                  >
                    <ShareLinkIcon /> {w.is_public ? t('wb.sharedOn') : t('wb.share')}
                  </button>
                  <button className="chip-btn danger" onClick={() => void remove(w)} title={t('common.delete')}>
                    <TrashIcon />
                  </button>
                </div>
              </div>
            </article>
          ))}
        </div>
      )}

      {view && (
        <div className="modal-overlay" onClick={() => setView(null)}>
          <div className="modal wb-viewer" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>{view.title}</h3>
              <button className="panel-close" onClick={() => setView(null)}><CloseIcon /></button>
            </div>
            <FullBoard
              id={view.id}
              title={view.title}
              loadingLabel={t('common.loading')}
              downloadLabel={t('recordings.download')}
            />
          </div>
        </div>
      )}
    </div>
  )
}
