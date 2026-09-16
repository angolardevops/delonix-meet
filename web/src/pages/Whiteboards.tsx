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
 */
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, deleteWhiteboard, listWhiteboards, shareWhiteboard, WhiteboardMeta } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Icon } from '../ui/icons'
import { Alert, Button, cx, Dialog, Empty, IconButton, Skeleton, TextInput } from '../ui/kit'
import '../ui/boards.css'
import BoardCard from './boards/BoardCard'
import BoardViewer, { boardShareUrl } from './boards/BoardViewer'
import LocalDiagrams from './diagrams/LocalDiagrams'

type Filter = 'all' | 'public' | 'private'

export default function Whiteboards() {
  const { t } = useTranslation()
  const { enterRoom } = useShell()
  const { state, reload, mutate } = useAsync((signal) => listWhiteboards(signal), [])
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [viewId, setViewId] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [toDelete, setToDelete] = useState<WhiteboardMeta | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [deleteErr, setDeleteErr] = useState('')
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; text: string } | null>(null)

  const items = useMemo(() => (state.s === 'ready' ? state.d : []), [state])
  const viewing = items.find((b) => b.id === viewId) ?? null
  const publicCount = items.filter((b) => b.is_public).length

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return items.filter((b) => {
      if (filter === 'public' && !b.is_public) return false
      if (filter === 'private' && b.is_public) return false
      if (!q) return true
      return b.title.toLowerCase().includes(q) || b.room_code.toLowerCase().includes(q)
    })
  }, [items, query, filter])

  const toggleShare = useCallback(
    async (b: WhiteboardMeta) => {
      setBusyId(b.id)
      setNotice(null)
      try {
        const updated = await shareWhiteboard(b.id, !b.is_public)
        mutate((list) => list.map((x) => (x.id === b.id ? updated : x)))
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

  const filters: { value: Filter; label: string; count: number }[] = [
    { value: 'all', label: t('boards.filtros.todos'), count: items.length },
    { value: 'public', label: t('boards.filtros.publicos'), count: publicCount },
    { value: 'private', label: t('boards.filtros.privados'), count: items.length - publicCount },
  ]

  return (
    <>
      <PageBar
        title={t('boards.titulo')}
        meta={state.s === 'ready' ? t('boards.meta', { count: items.length, publicos: publicCount }) : undefined}
      >
        <label className="board-search">
          <Icon name="search" size={14} />
          <TextInput
            type="search"
            autoComplete="off"
            value={query}
            placeholder={t('boards.pesquisa.placeholder')}
            aria-label={t('boards.pesquisa.rotulo')}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <Button size="sm" variant="primary" icon="plus" onClick={() => (location.hash = '/whiteboards/diagram')}>
          {t('diagrams.novo')}
        </Button>
      </PageBar>

      <div className="page board-page">
        <LocalDiagrams />
        <div className="dx-chips" role="group" aria-label={t('boards.filtros.rotulo')}>
          {filters.map((f) => (
            <button
              key={f.value}
              type="button"
              className="dx-chip"
              aria-pressed={filter === f.value}
              onClick={() => setFilter(f.value)}
            >
              {f.label}
              <span className="dx-num">{f.count}</span>
            </button>
          ))}
        </div>

        {notice && (
          <div className={cx('board-notice', notice.tone === 'danger' && 'is-danger')} role="status">
            <Alert tone={notice.tone} icon={notice.tone === 'danger' ? 'alert' : 'check'}>
              {notice.text}
            </Alert>
            <IconButton icon="x" bare label={t('ui.dispensar')} onClick={() => setNotice(null)} />
          </div>
        )}

        <AsyncSection
          state={state}
          onRetry={reload}
          skeleton={
            <div className="board-grid" aria-busy="true">
              {[0, 1, 2, 3].map((i) => (
                <Skeleton key={i} h={190} />
              ))}
            </div>
          }
        >
          {() =>
            items.length === 0 ? (
              <Empty icon="board" title={t('boards.vazio.titulo')}>
                {t('boards.vazio.texto')}
              </Empty>
            ) : shown.length === 0 ? (
              <Empty
                icon="search"
                title={t('boards.semResultados')}
                action={
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      setQuery('')
                      setFilter('all')
                    }}
                  >
                    {t('boards.limparFiltros')}
                  </Button>
                }
              />
            ) : (
              <ul className="board-grid">
                {shown.map((b) => (
                  <BoardCard
                    key={b.id}
                    board={b}
                    busy={busyId === b.id}
                    onView={view}
                    onToggleShare={(x) => void toggleShare(x)}
                    onOpenRoom={enterRoom}
                    onDelete={askDelete}
                  />
                ))}
              </ul>
            )
          }
        </AsyncSection>
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
