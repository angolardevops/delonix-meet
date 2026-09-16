/**
 * Gravações — «Gravações e videoaulas» (DelonixRecordings): barra com contagem
 * e pesquisa, Lista/Grelha, chips de filtro, a tabela de seis colunas e o
 * painel direito com o leitor.
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
import { recordingsLibrary } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Icon } from '../ui/icons'
import { Button, cx, Empty, Segmented, Skeleton, TextInput } from '../ui/kit'
import '../ui/recordings.css'
import { formatBytes } from './recordings/format'
import { filterCounts, LibraryFilter, matchesFilter, visibleFilters } from './recordings/libraryData'
import RecordingGrid from './recordings/RecordingGrid'
import RecordingPanel from './recordings/RecordingPanel'
import RecordingTable from './recordings/RecordingTable'
import { fromRecordingItem, RecordingView } from './recordings/recordingView'
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

const FILTER_LABEL: Record<LibraryFilter, string> = {
  all: 'recordings.filtros.todas',
  mine: 'recordings.filtros.minhas',
  shared: 'recordings.filtros.partilhadas',
  training: 'recordings.filtros.videoaulas',
  broadcast: 'recordings.filtros.emissoes',
  meeting: 'recordings.filtros.reunioes',
  '4k': 'recordings.filtros.quatroK',
  processing: 'recordings.filtros.aProcessar',
  failed: 'recordings.filtros.falhadas',
}

export default function Recordings() {
  const { t, i18n } = useTranslation()
  const { org } = useShell()
  const retentionDays = org?.retention_days ?? 0
  const { state, reload } = useAsync(async (signal) => (await recordingsLibrary(signal)).map(fromRecordingItem), [])
  const [view, setView] = useState<View>(storedView)
  const [query, setQuery] = useState(() => hashParam('q') ?? '')
  const [filter, setFilter] = useState<LibraryFilter>('all')
  // Seleccionada: o painel mostra-a. `picked` distingue a escolha da pessoa
  // (carrega o vídeo, e em ecrã estreito abre o painel por cima) da selecção
  // por omissão (primeira pronta, sem descarregar nada).
  // `#/recordings?id=<id>` (pesquisa global) abre já essa gravação.
  const [selectedId, setSelectedId] = useState<string | null>(() => hashParam('id'))
  const [picked, setPicked] = useState(() => hashParam('id') !== null)
  const [panelOpen, setPanelOpen] = useState(false)
  const [shareTarget, setShareTarget] = useState<RecordingView | null>(null)

  const items = useMemo(() => (state.s === 'ready' ? state.d : []), [state])

  function changeView(v: View) {
    setView(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* navegação privada: a vista não fica lembrada, e não faz mal */
    }
  }

  const counts = useMemo(() => filterCounts(items), [items])
  const chips = useMemo(() => visibleFilters(items), [items])
  const totalBytes = useMemo(() => items.reduce((n, r) => n + (r.sizeBytes ?? 0), 0), [items])

  // Um filtro que deixou de ter dado (ex.: a última falhada saiu) volta a «Todas».
  useEffect(() => {
    if (state.s === 'ready' && !chips.includes(filter)) setFilter('all')
  }, [state.s, chips, filter])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return items.filter((r) => {
      if (!matchesFilter(r, filter)) return false
      if (!q) return true
      return r.name.toLowerCase().includes(q) || r.roomCode.toLowerCase().includes(q) || r.uploaderName.toLowerCase().includes(q)
    })
  }, [items, query, filter])

  // Selecção por omissão: a primeira PRONTA. Uma falhada nunca é seleccionada.
  useEffect(() => {
    if (state.s !== 'ready') return
    const current = items.find((r) => r.id === selectedId)
    if (current && !current.failed) return
    const first = items.find((r) => !r.failed)
    setSelectedId(first?.id ?? null)
    setPicked(false)
  }, [state.s, items, selectedId])

  const selected = items.find((r) => r.id === selectedId && !r.failed) ?? null

  const open = useCallback((r: RecordingView) => {
    if (r.failed) return
    setSelectedId(r.id)
    setPicked(true)
    setPanelOpen(true)
  }, [])
  const closePanel = useCallback(() => setPanelOpen(false), [])
  // Estável: o `Dialog` re-foca quando o `onClose` muda.
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

  return (
    <>
      <PageBar
        title={t('recordings.titulo')}
        meta={state.s === 'ready' ? t('recordings.meta', { count: items.length, size: formatBytes(totalBytes, i18n.language) }) : undefined}
      >
        <label className="rec-search">
          <Icon name="search" size={14} />
          <TextInput
            type="search"
            autoComplete="off"
            value={query}
            placeholder={t('recordings.pesquisa.placeholder')}
            aria-label={t('recordings.pesquisa.rotulo')}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
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
          <div className="rec-chips" role="group" aria-label={t('recordings.filtros.rotulo')}>
            {chips.map((f) => {
              // Como o template: a contagem só se vê nos chips de atenção.
              const attention = f === 'processing' || f === 'failed'
              return (
                <button
                  key={f}
                  type="button"
                  className={cx('rec-chip', attention && `rec-chip--${f}`)}
                  aria-pressed={filter === f}
                  onClick={() => setFilter(f)}
                >
                  {t(FILTER_LABEL[f])}
                  {attention ? (
                    <span className="dx-num"> · {counts[f]}</span>
                  ) : (
                    <span className="dx-sr-only">{t('recordings.filtros.contagem', { count: counts[f] })}</span>
                  )}
                </button>
              )
            })}
          </div>

          <AsyncSection
            state={state}
            onRetry={reload}
            skeleton={
              <div className="rec-skeleton" aria-busy="true">
                {[0, 1, 2, 3, 4].map((i) => (
                  <Skeleton key={i} h={44} />
                ))}
              </div>
            }
          >
            {() =>
              items.length === 0 ? (
                <Empty icon="film" title={t('recordings.vazio.titulo')}>
                  {t('recordings.vazio.texto')}
                </Empty>
              ) : shown.length === 0 ? (
                <Empty
                  icon="search"
                  title={t('recordings.semResultados')}
                  action={
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setQuery('')
                        setFilter('all')
                      }}
                    >
                      {t('recordings.limparFiltros')}
                    </Button>
                  }
                />
              ) : view === 'list' ? (
                <RecordingTable items={shown} selectedId={selected?.id ?? null} retentionDays={retentionDays} onOpen={open} />
              ) : (
                <RecordingGrid items={shown} selectedId={selected?.id ?? null} retentionDays={retentionDays} onOpen={open} />
              )
            }
          </AsyncSection>
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
