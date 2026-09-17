/**
 * Gravações — «Gravações e videoaulas» (DelonixRecordings): barra com contagem
 * e pesquisa, Lista/Grelha, chips de filtro, a tabela de seis colunas e o
 * painel direito com o leitor.
 *
 * Pesquisa, filtros, agrupamentos e página: o painel estilo Odoo
 * (`ui/search`) sobre o recurso `recordings` do servidor — ou, se o servidor
 * ainda não o tiver, sobre a biblioteca inteira, dito no ecrã.
 *
 * Os componentes só vêem `RecordingView` (`recordings/recordingView.ts`), a
 * camada que lê a API. Hoje a biblioteca não traz duração, resolução,
 * categoria, estados de processamento nem armazenamento por gravação: as
 * colunas existem e mostram «—», e os chips que filtrariam por esses campos
 * não aparecem (filtrariam sempre para zero). Fica de fora, sem botão inerte:
 * «Enviar para storage», «Publicar», «Exportar», «Guardar em…», pesquisa na
 * transcrição e o cartão MinIO/Nextcloud.
 *
 * R59: uma gravação FALHADA aparece com a causa, mas nunca é seleccionável,
 * nunca abre o leitor e nunca oferece acções — em NENHUMA das vistas. O e2e
 * `gravacao-falhada.mjs` verifica as duas.
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RecordingItem } from '../api'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { cx, Segmented, Skeleton } from '../ui/kit'
import { SearchBar, SearchResults } from '../ui/search/SearchResults'
import { useResourceSearch } from '../ui/search/useResourceSearch'
import '../ui/recordings.css'
import { formatBytes } from './recordings/format'
import { formatClock } from './recordings/libraryData'
import RecordingGrid from './recordings/RecordingGrid'
import RecordingPanel from './recordings/RecordingPanel'
import RecordingTable from './recordings/RecordingTable'
import { fromRecordingItem, RecordingView } from './recordings/recordingView'
import { recordingsFallback } from './recordings/search'
import ShareDialog from './recordings/ShareDialog'

type View = 'list' | 'grid'

const VIEW_KEY = 'dx_rec_view'

function storedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'grid' ? 'grid' : 'list'
  } catch {
    return 'list'
  }
}

function hashParam(name: string): string | null {
  const i = location.hash.indexOf('?')
  return i < 0 ? null : new URLSearchParams(location.hash.slice(i + 1)).get(name)
}

export default function Recordings() {
  const { t, i18n } = useTranslation()
  const { org } = useShell()
  const retentionDays = org?.retention_days ?? 0
  const rs = useResourceSearch<RecordingItem>({ resource: 'recordings', fallback: recordingsFallback })
  const [view, setView] = useState<View>(storedView)
  // Seleccionada: o painel mostra-a. `picked` distingue a escolha da pessoa
  // (carrega o vídeo, e em ecrã estreito abre o painel por cima) da selecção
  // por omissão (primeira pronta, sem descarregar nada).
  // `#/recordings?id=<id>` abre já essa gravação, se estiver na página.
  const [selectedId, setSelectedId] = useState<string | null>(() => hashParam('id'))
  const [picked, setPicked] = useState(() => hashParam('id') !== null)
  const [panelOpen, setPanelOpen] = useState(false)
  const [shareTarget, setShareTarget] = useState<RecordingView | null>(null)

  const page = rs.list.state.s === 'ready' ? rs.list.state.d : null
  const items = useMemo(() => (page ? page.items.map(fromRecordingItem) : []), [page])
  const ready = page !== null

  function changeView(v: View) {
    setView(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* navegação privada: a vista não fica lembrada, e não faz mal */
    }
  }

  // Selecção por omissão: a primeira PRONTA da página. Uma falhada nunca é seleccionada.
  useEffect(() => {
    if (!ready) return
    const current = items.find((r) => r.id === selectedId)
    if (current && !current.failed) return
    const first = items.find((r) => !r.failed)
    setSelectedId(first?.id ?? null)
    setPicked(false)
  }, [ready, items, selectedId])

  const selected = items.find((r) => r.id === selectedId && !r.failed) ?? null

  const open = useCallback((r: RecordingView) => {
    if (r.failed) return
    setSelectedId(r.id)
    setPicked(true)
    setPanelOpen(true)
  }, [])
  const closePanel = useCallback(() => setPanelOpen(false), [])
  // Estável: o `Dialog` re-foca quando o `onClose` muda.
  const reload = rs.reload
  const closeShare = useCallback(() => {
    setShareTarget(null)
    reload()
  }, [reload])

  // Em ecrã estreito o painel é uma camada: Esc fecha-o.
  useEffect(() => {
    if (!panelOpen || shareTarget) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setPanelOpen(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [panelOpen, shareTarget])

  const renderItems = (rows: RecordingItem[]) => {
    const views = rows.map(fromRecordingItem)
    return view === 'list' ? (
      <RecordingTable items={views} selectedId={selected?.id ?? null} retentionDays={retentionDays} onOpen={open} />
    ) : (
      <RecordingGrid items={views} selectedId={selected?.id ?? null} retentionDays={retentionDays} onOpen={open} />
    )
  }

  return (
    <>
      <PageBar
        title={t('recordings.titulo')}
        meta={page ? t('search.grupos.registos', { count: page.total }) + (page.total_kind === 'at_least' ? '+' : '') : undefined}
      >
        <div className="rec-views">
          <Segmented<View>
            label={t('recordings.vistas.rotulo')}
            value={view}
            onChange={changeView}
            options={[
              { value: 'list', label: t('recordings.vistas.lista') },
              { value: 'grid', label: t('recordings.vistas.grelha') },
            ]}
          />
        </div>
      </PageBar>

      <div className={cx('rec-layout', !selected && 'is-single')}>
        <div className="rec-main">
          <SearchBar rs={rs} label={t('recordings.pesquisa.rotulo')} placeholder={t('recordings.pesquisa.placeholder')} />
          <div className="rec-results">
            <SearchResults
              rs={rs}
              renderItems={renderItems}
              emptyIcon="film"
              emptyTitle={t('recordings.vazio.titulo')}
              emptyText={t('recordings.vazio.texto')}
              formatAggregate={(field, _kind, v) =>
                field === 'size_bytes' ? formatBytes(v, i18n.language) : field === 'duration_secs' ? formatClock(v * 1000) : v.toLocaleString(i18n.language)
              }
              skeleton={
                <div className="rec-skeleton" aria-busy="true">
                  {[0, 1, 2, 3, 4].map((i) => (
                    <Skeleton key={i} h={44} />
                  ))}
                </div>
              }
            />
          </div>
        </div>

        {selected && (
          <aside className={cx('rec-panel', panelOpen && 'is-open')} aria-label={t('recordings.leitor.rotulo')}>
            <RecordingPanel key={selected.id} rec={selected} autoLoad={picked} onShare={setShareTarget} onClose={closePanel} />
          </aside>
        )}
        {selected && panelOpen && <div className="rec-panel-scrim" onClick={closePanel} aria-hidden="true" />}
      </div>

      {shareTarget && <ShareDialog rec={shareTarget.source} onClose={closeShare} />}
    </>
  )
}
