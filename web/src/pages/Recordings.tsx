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
 * camada que lê a API: duração e resolução medidas, categoria, estados,
 * miniatura. «Minhas» e «Publicadas» são o `scope` do servidor. Publicar,
 * editar, capítulos, comentários e legendas vivem no leitor. Fica de fora, sem
 * botão inerte: armazenamento por gravação, «Enviar para storage»,
 * «Exportar», «Guardar em…» e o cartão MinIO/Nextcloud.
 *
 * R59: uma gravação FALHADA aparece com a causa, mas nunca é seleccionável,
 * nunca abre o leitor e nunca oferece acções — em NENHUMA das vistas. O e2e
 * `gravacao-falhada.mjs` verifica as duas.
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RecordingLibraryItem } from '../api'
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
import { recordingsFallbackFor } from './recordings/search'
import ShareDialog from './recordings/ShareDialog'
import { useProcessingUpdates } from './recordings/useProcessingUpdates'

type View = 'list' | 'grid'
const NO_ITEMS: RecordingLibraryItem[] = []

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

/**
 * Escreve (ou tira) um parâmetro no endereço, SEM empilhar no histórico.
 *
 * Porque é preciso: o âmbito era LIDO do endereço à entrada e nunca lá escrito
 * ao mudar. Quem recebia `#/recordings?scope=published`, clicava em «Minhas» e
 * voltava a partilhar o link, partilhava um endereço que dizia «publicadas» a
 * mostrar as dele — o URL mentia. E um F5 desfazia a escolha.
 *
 * `replaceState` e não `location.hash`: mudar de separador não é navegar, e não
 * deve gastar uma entrada do «voltar».
 */
function porNoEndereco(nome: string, valor: string | null): void {
  const i = location.hash.indexOf('?')
  const base = i < 0 ? location.hash : location.hash.slice(0, i)
  const p = new URLSearchParams(i < 0 ? '' : location.hash.slice(i + 1))
  if (valor === null) p.delete(nome)
  else p.set(nome, valor)
  const q = p.toString()
  const alvo = q ? `${base}?${q}` : base
  if (location.hash !== alvo) history.replaceState(null, '', alvo)
}

export default function Recordings() {
  const { t, i18n } = useTranslation()
  const { org } = useShell()
  const retentionDays = org?.retention_days ?? 0
  // «Minhas» (participei, partilhadas comigo) ou «Publicadas» na organização.
  const [scope, setScope] = useState<'mine' | 'published'>(() => (hashParam('scope') === 'published' ? 'published' : 'mine'))
  const fallback = useMemo(() => recordingsFallbackFor(scope), [scope])
  // Com a pesquisa do servidor, o `scope` vai em todos os pedidos da lista:
  // sem ele, «Publicadas» mostrava a biblioteca pessoal (R235).
  const serverParams = useMemo(() => ({ scope }), [scope])
  const rs = useResourceSearch<RecordingLibraryItem>({ resource: 'recordings', fallback, serverParams, deps: [scope] })
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
  // As que o servidor ainda está a compor relêem-se à parte (progresso, e a
  // passagem a «pronta»), sem voltar a pedir a lista.
  const live = useProcessingUpdates(page?.items ?? NO_ITEMS)
  const items = useMemo(() => (page ? page.items.map(live).map(fromRecordingItem) : []), [page, live])
  const ready = page !== null

  function changeView(v: View) {
    setView(v)
    try {
      localStorage.setItem(VIEW_KEY, v)
    } catch {
      /* navegação privada: a vista não fica lembrada, e não faz mal */
    }
  }

  // Selecção por omissão: a primeira COM FICHEIRO da página. Uma falhada ou
  // uma que ainda está a compor nunca é seleccionada.
  useEffect(() => {
    if (!ready) return
    const current = items.find((r) => r.id === selectedId)
    if (current?.hasFile) return
    const first = items.find((r) => r.hasFile)
    setSelectedId(first?.id ?? null)
    setPicked(false)
  }, [ready, items, selectedId])

  const selected = items.find((r) => r.id === selectedId && r.hasFile) ?? null

  const open = useCallback((r: RecordingView) => {
    if (!r.hasFile) return
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

  const renderItems = (rows: RecordingLibraryItem[]) => {
    const views = rows.map(live).map(fromRecordingItem)
    return view === 'list' ? (
      <RecordingTable items={views} selectedId={selected?.id ?? null} retentionDays={retentionDays} onOpen={open} onShare={setShareTarget} />
    ) : (
      <RecordingGrid items={views} selectedId={selected?.id ?? null} retentionDays={retentionDays} onOpen={open} onShare={setShareTarget} />
    )
  }

  return (
    <>
      <PageBar
        title={t('recordings.titulo')}
        meta={page ? t('search.grupos.registos', { count: page.total }) + (page.total_kind === 'at_least' ? '+' : '') : undefined}
      >
        {/* Âmbito e vista em contentores SEPARADOS: o `.rec-views` é o selector
            de vista e mais nada. Juntos, um `.rec-views button` deixava de
            querer dizer «lista/grelha» — foi o que partiu o e2e da gravação
            falhada, que clica no segundo botão para ir à grelha. */}
        <div className="rec-scope">
          <Segmented<'mine' | 'published'>
            label={t('recordings.ambito.rotulo')}
            value={scope}
            onChange={(v) => {
              setScope(v)
              // O endereço passa a dizer a verdade — e sobrevive ao F5.
              porNoEndereco('scope', v === 'published' ? 'published' : null)
            }}
            options={[
              { value: 'mine', label: t('recordings.ambito.minhas') },
              { value: 'published', label: t('recordings.ambito.publicadas') },
            ]}
          />
        </div>
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
