/**
 * Quadros — a biblioteca dos quadros brancos guardados na organização.
 *
 * O template só tem o quadro AO VIVO («Quadro partilhado no palco»), que é da
 * sala. Daqui ficam a gramática da consola (barra, chips, cartões) e o que a
 * API sustenta: pré-visualização PNG, link público só-leitura, abrir a sala de
 * origem e eliminar. «Guardar no storage», «Anexar à gravação», páginas,
 * caneta e cursores não têm endpoint e não aparecem.
 *
 * Quem pode partilhar ou eliminar decide o servidor (dono ou admin da org);
 * a recusa chega como a mensagem que ele devolve.
 *
 * Pesquisa, filtros, agrupar e página: o painel estilo Odoo (recurso
 * `whiteboards`; sem ele no servidor, a lista inteira filtrada no browser).
 * `#/whiteboards?id=<id>` (pesquisa global) abre o quadro.
 */
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, deleteWhiteboard, listWhiteboards, shareWhiteboard, WhiteboardMeta } from '../api'
import { useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Alert, Button, cx, Dialog, IconButton, Skeleton } from '../ui/kit'
import { hashParams } from '../ui/search/model'
import { SearchBar, SearchResults } from '../ui/search/SearchResults'
import { useResourceSearch } from '../ui/search/useResourceSearch'
import '../ui/boards.css'
import BoardCard from './boards/BoardCard'
import BoardViewer, { boardShareUrl } from './boards/BoardViewer'
import { whiteboardsFallback } from './boards/search'
import LocalDiagrams from './diagrams/LocalDiagrams'

export default function Whiteboards() {
  const { t } = useTranslation()
  const { enterRoom } = useShell()
  // A lista inteira de sempre: contagens da barra e o quadro aberto por id.
  const { state, reload: reloadAll, mutate } = useAsync((signal) => listWhiteboards(signal), [])
  const rs = useResourceSearch<WhiteboardMeta>({ resource: 'whiteboards', fallback: whiteboardsFallback })
  const reloadSearch = rs.reload
  const reload = useCallback(() => {
    reloadAll()
    reloadSearch()
  }, [reloadAll, reloadSearch])
  // Partilhar muda um quadro: a página da pesquisa mostra-o já actualizado.
  const [updated, setUpdated] = useState<Record<string, WhiteboardMeta>>({})
  const [viewId, setViewId] = useState<string | null>(() => hashParams().get('id'))
  const [busyId, setBusyId] = useState<string | null>(null)
  const [toDelete, setToDelete] = useState<WhiteboardMeta | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [deleteErr, setDeleteErr] = useState('')
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; text: string } | null>(null)

  const items = useMemo(() => (state.s === 'ready' ? state.d : []), [state])
  const page = rs.list.state.s === 'ready' ? rs.list.state.d.items : []
  const viewing = updated[viewId ?? ''] ?? items.find((b) => b.id === viewId) ?? page.find((b) => b.id === viewId) ?? null
  const publicCount = items.filter((b) => b.is_public).length

  const toggleShare = useCallback(
    async (b: WhiteboardMeta) => {
      setBusyId(b.id)
      setNotice(null)
      try {
        const updated = await shareWhiteboard(b.id, !b.is_public)
        mutate((list) => list.map((x) => (x.id === b.id ? updated : x)))
        setUpdated((m) => ({ ...m, [updated.id]: updated }))
        if (updated.is_public && updated.share_token) {
          // Tornar público é quase sempre para mandar a alguém: copia-se já.
          const copied = await navigator.clipboard
            .writeText(boardShareUrl(updated.share_token))
            .then(() => true)
            .catch(() => false)
          setNotice({ tone: 'success', text: copied ? t('boards.aviso.linkCopiado') : t('boards.aviso.linkCriado') })
        } else if (!updated.is_public) {
          setNotice({ tone: 'success', text: t('boards.aviso.linkDesligado') })
        }
      } catch (e) {
        setNotice({ tone: 'danger', text: apiErrorMessage(e, t('boards.aviso.erroPartilha')) })
      } finally {
        setBusyId(null)
      }
    },
    [mutate, t],
  )

  const askDelete = useCallback((b: WhiteboardMeta) => {
    setDeleteErr('')
    setToDelete(b)
  }, [])
  const closeDelete = useCallback(() => setToDelete(null), [])
  const closeViewer = useCallback(() => setViewId(null), [])
  const view = useCallback((b: WhiteboardMeta) => setViewId(b.id), [])

  async function confirmDelete() {
    if (!toDelete) return
    setDeleting(true)
    setDeleteErr('')
    try {
      await deleteWhiteboard(toDelete.id)
      const id = toDelete.id
      mutate((list) => list.filter((x) => x.id !== id))
      if (viewId === id) setViewId(null)
      setToDelete(null)
      reload()
    } catch (e) {
      setDeleteErr(apiErrorMessage(e, t('boards.eliminar.erro')))
    } finally {
      setDeleting(false)
    }
  }


  return (
    <>
      <PageBar
        title={t('boards.titulo')}
        meta={state.s === 'ready' ? t('boards.meta', { count: items.length, publicos: publicCount }) : undefined}
      >
        <Button size="sm" variant="primary" icon="plus" onClick={() => (location.hash = '/whiteboards/diagram')}>
          {t('diagrams.novo')}
        </Button>
      </PageBar>

      <div className="page board-page">
        <LocalDiagrams />
        <SearchBar rs={rs} label={t('boards.pesquisa.rotulo')} placeholder={t('boards.pesquisa.placeholder')} />

        {notice && (
          <div className={cx('board-notice', notice.tone === 'danger' && 'is-danger')} role="status">
            <Alert tone={notice.tone} icon={notice.tone === 'danger' ? 'alert' : 'check'}>
              {notice.text}
            </Alert>
            <IconButton icon="x" bare label={t('ui.dispensar')} onClick={() => setNotice(null)} />
          </div>
        )}

        <SearchResults
          rs={rs}
          emptyIcon="board"
          emptyTitle={t('boards.vazio.titulo')}
          emptyText={t('boards.vazio.texto')}
          skeleton={
            <div className="board-grid" aria-busy="true">
              {[0, 1, 2, 3].map((i) => (
                <Skeleton key={i} h={190} />
              ))}
            </div>
          }
          renderItems={(rows) => (
            <ul className="board-grid">
              {rows.map((raw) => {
                const b = updated[raw.id] ?? raw
                return (
                  <BoardCard
                    key={b.id}
                    board={b}
                    busy={busyId === b.id}
                    onView={view}
                    onToggleShare={(x) => void toggleShare(x)}
                    onOpenRoom={enterRoom}
                    onDelete={askDelete}
                  />
                )
              })}
            </ul>
          )}
        />
      </div>

      {viewing && !toDelete && (
        <BoardViewer
          board={viewing}
          busy={busyId === viewing.id}
          onClose={closeViewer}
          onToggleShare={(x) => void toggleShare(x)}
          onOpenRoom={enterRoom}
          onDelete={askDelete}
        />
      )}

      {toDelete && (
        <Dialog
          title={t('boards.eliminar.titulo')}
          onClose={closeDelete}
          footer={
            <>
              <Button variant="ghost" disabled={deleting} onClick={closeDelete}>
                {t('ui.cancelar')}
              </Button>
              <Button variant="danger" icon="trash" busy={deleting} onClick={() => void confirmDelete()}>
                {t('boards.eliminar.confirmar')}
              </Button>
            </>
          }
        >
          <p className="board-delete__text">
            {t('boards.eliminar.texto', { title: toDelete.title || t('boards.semTitulo') })}
          </p>
          {deleteErr && <Alert tone="danger">{deleteErr}</Alert>}
        </Dialog>
      )}
    </>
  )
}
