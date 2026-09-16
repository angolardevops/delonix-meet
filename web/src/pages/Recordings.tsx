/**
 * Gravações — «Gravações e videoaulas» do template: barra com contagem e
 * pesquisa, Lista/Grelha, filtros em chips, e o painel direito com o leitor.
 *
 * O que o template mostra e a API não tem fica de fora, sem botão inerte nem
 * número inventado: «Enviar para storage», «Publicar», «Exportar»,
 * «Guardar em…», capítulos automáticos, categorias (videoaulas/emissões),
 * resolução 4K, estados «a processar/a transcrever/retida», armazenamento
 * MinIO/Nextcloud e pesquisa na transcrição. A contagem e o tamanho total da
 * barra são somados do que o servidor devolve.
 *
 * R59: uma gravação FALHADA aparece (antes desaparecia em silêncio) com a
 * causa, mas nunca é seleccionável, nunca abre o leitor e nunca oferece
 * acções — em NENHUMA das vistas. O e2e `gravacao-falhada.mjs` verifica as
 * duas.
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RecordingItem, recordingsLibrary } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { Icon } from '../ui/icons'
import { Button, cx, Empty, Segmented, Skeleton, TextInput } from '../ui/kit'
import '../ui/recordings.css'
import { formatBytes, isFailed, recordingName } from './recordings/format'
import RecordingGrid from './recordings/RecordingGrid'
import RecordingPanel from './recordings/RecordingPanel'
import RecordingTable from './recordings/RecordingTable'
import ShareDialog from './recordings/ShareDialog'

type View = 'list' | 'grid'
type Filter = 'all' | 'mine' | 'shared' | 'failed'

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
  const { state, reload } = useAsync((signal) => recordingsLibrary(signal), [])
  const [view, setView] = useState<View>(storedView)
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  // Seleccionada: o painel mostra-a. `picked` distingue a escolha da pessoa
  // (carrega o vídeo, e em ecrã estreito abre o painel por cima) da selecção
  // por omissão (primeira pronta, sem descarregar nada).
  // `#/recordings?id=<id>` (pesquisa global) abre já essa gravação.
  const [selectedId, setSelectedId] = useState<string | null>(() => hashParam('id'))
  const [picked, setPicked] = useState(() => hashParam('id') !== null)
  const [panelOpen, setPanelOpen] = useState(false)
  const [shareTarget, setShareTarget] = useState<RecordingItem | null>(null)

  const items = useMemo(() => (state.s === 'ready' ? state.d : []), [state])

  function changeView(v: View) {
    setView(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* navegação privada: a vista não fica lembrada, e não faz mal */
    }
  }

  const counts = useMemo(
    () => ({
      all: items.length,
      mine: items.filter((r) => r.owned).length,
      shared: items.filter((r) => !r.owned).length,
      failed: items.filter(isFailed).length,
      bytes: items.reduce((n, r) => n + (isFailed(r) ? 0 : r.size_bytes), 0),
    }),
    [items],
  )

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return items.filter((r) => {
      if (filter === 'mine' && !r.owned) return false
      if (filter === 'shared' && r.owned) return false
      if (filter === 'failed' && !isFailed(r)) return false
      if (!q) return true
      return (
        recordingName(r).toLowerCase().includes(q) ||
        r.room_code.toLowerCase().includes(q) ||
        r.uploader_name.toLowerCase().includes(q)
      )
    })
  }, [items, query, filter])

  // Selecção por omissão: a primeira PRONTA. Uma falhada nunca é seleccionada.
  useEffect(() => {
    if (state.s !== 'ready') return
    const current = items.find((r) => r.id === selectedId)
    if (current && !isFailed(current)) return
    const first = items.find((r) => !isFailed(r))
    setSelectedId(first?.id ?? null)
    setPicked(false)
  }, [state.s, items, selectedId])

  const selected = items.find((r) => r.id === selectedId && !isFailed(r)) ?? null

  const open = useCallback((r: RecordingItem) => {
    if (isFailed(r)) return
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

  const filters: { value: Filter; label: string; count: number }[] = [
    { value: 'all', label: t('recordings.filtros.todas'), count: counts.all },
    { value: 'mine', label: t('recordings.filtros.minhas'), count: counts.mine },
    { value: 'shared', label: t('recordings.filtros.partilhadas'), count: counts.shared },
  ]
  if (counts.failed > 0) filters.push({ value: 'failed', label: t('recordings.filtros.falhadas'), count: counts.failed })

  return (
    <>
      <PageBar
        title={t('recordings.titulo')}
        meta={
          state.s === 'ready'
            ? t('recordings.meta', { count: counts.all, size: formatBytes(counts.bytes, i18n.language) })
            : undefined
        }
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
        <div className="page rec-main">
          <div className="dx-chips" role="group" aria-label={t('recordings.filtros.rotulo')}>
            {filters.map((f) => (
              <button
                key={f.value}
                type="button"
                className={cx('dx-chip', f.value === 'failed' && 'rec-chip--failed')}
                aria-pressed={filter === f.value}
                onClick={() => setFilter(f.value)}
              >
                {f.label}
                <span className="dx-num">{f.count}</span>
              </button>
            ))}
          </div>

          <AsyncSection
            state={state}
            onRetry={reload}
            skeleton={
              <div className="rec-skeleton" aria-busy="true">
                {[0, 1, 2, 3, 4].map((i) => (
                  <Skeleton key={i} h={38} />
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
                <RecordingTable items={shown} selectedId={selected?.id ?? null} onOpen={open} />
              ) : (
                <RecordingGrid items={shown} selectedId={selected?.id ?? null} onOpen={open} />
              )
            }
          </AsyncSection>
        </div>

        {selected && (
          <aside className={cx('rec-panel', panelOpen && 'is-open')} aria-label={t('recordings.leitor.rotulo')}>
            <RecordingPanel
              key={selected.id}
              rec={selected}
              autoLoad={picked}
              onShare={setShareTarget}
              onClose={closePanel}
            />
          </aside>
        )}
        {selected && panelOpen && <div className="rec-panel-scrim" onClick={closePanel} aria-hidden="true" />}
      </div>

      {shareTarget && <ShareDialog rec={shareTarget} onClose={closeShare} />}
    </>
  )
}
