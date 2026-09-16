/**
 * Editor do Estúdio — «Linha de tempo», «Legendas e tradução» e «Exportações».
 *
 * Um projecto NÃO DESTRUTIVO em IndexedDB (`edit/projecto.ts`, `edit/bd.ts`):
 * as fontes gravadas ou importadas nunca são alteradas; cortar, mover, mudar a
 * cor ou a mistura é acrescentar uma edição à lista, e desfazer é voltar um
 * passo. O ficheiro só nasce na exportação (`edit/render.ts`).
 *
 * A página (`pages/Studio.tsx`) continua dona da gravação e do envio para a
 * biblioteca (grava primeiro no dispositivo, depois no servidor); este painel
 * recebe a gravação acabada e devolve o ficheiro exportado.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort, recordingObjectUrl, recordingsLibrary } from '../api'
import type { RecordingItem } from '../api'
import { getAppName } from '../branding'
import { useShell } from '../components/shellContext'
import { DelonixSymbol, Icon } from '../ui/icons'
import type { IconName } from '../ui/icons'
import { Alert, Button, cx, Dialog, Empty, IconButton, TextInput } from '../ui/kit'
import '../ui/editor.css'
import { analisarPausas } from './analise'
import CaptionsPanel from './captions/CaptionsPanel'
import { contarPreenchimento, encontrarPreenchimento, palavrasDasCues, relogio } from './captions/legendas'
import type { ResultadoDaGravacao } from './compositor'
import Bin, { Biblioteca } from './edit/Bin'
import type { AbaDoBin, ResumoDePausas } from './edit/Bin'
import Inspector from './edit/Inspector'
import type { AbaDoInspector } from './edit/Inspector'
import { ondaDe } from './edit/midia'
import Preview from './edit/Preview'
import type { Intervalo } from './edit/projecto'
import { clipsDaFaixa, duracaoDoProjecto, intervalosDaFonteNaLinha, novoId, normalizarIntervalos } from './edit/projecto'
import Timeline from './edit/Timeline'
import type { Ferramenta } from './edit/Timeline'
import { useLeitor } from './edit/useLeitor'
import { useProjecto } from './edit/useProjecto'
import type { FonteEmBruto } from './edit/useProjecto'
import ExportsPanel from './exports/ExportsPanel'

export interface Gravado {
  faixas: ResultadoDaGravacao
  url: string
  duracao: number
}

export type VistaDoEditor = 'edicao' | 'legendas' | 'exportacoes'

const FERRAMENTAS: { id: Ferramenta; icone: IconName }[] = [
  { id: 'seleccionar', icone: 'arrow' },
  { id: 'lamina', icone: 'scissors' },
  { id: 'aparar', icone: 'columns' },
  { id: 'deslizar', icone: 'repeat' },
  { id: 'transicao', icone: 'layers' },
  { id: 'texto', icone: 'text' },
  { id: 'mascara', icone: 'square' },
  { id: 'audio', icone: 'volume' },
]

/** `#/studio?vista=legendas&gravacao=<id>` — a gravação da biblioteca a abrir no editor. */
function gravacaoDoEndereco(): string | null {
  return new URLSearchParams(location.hash.split('?')[1] ?? '').get('gravacao')
}

function semGravacaoNoEndereco() {
  const [rota, query = ''] = location.hash.split('?')
  const q = new URLSearchParams(query)
  if (!q.has('gravacao')) return
  q.delete('gravacao')
  const resto = q.toString()
  history.replaceState(null, '', resto ? `${rota}?${resto}` : rota)
}

/** Título do projecto a partir do nome do ficheiro da gravação. */
function tituloDaGravacao(nome: string): string {
  return nome.replace(/\.(webm|weba|mp4|mkv|mov)$/i, '')
}

function useAgora(ms: number): number {
  const [agora, setAgora] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setAgora(Date.now()), ms)
    return () => clearInterval(id)
  }, [ms])
  return agora
}

export default function EditPanel({
  vista,
  onVista,
  resultado,
  podeCortar,
  titulo,
  onTitulo,
  aGuardar,
  guardado,
  onGuardar,
}: {
  vista: VistaDoEditor
  onVista: (v: VistaDoEditor | 'emissao') => void
  resultado: Gravado | null
  podeCortar: boolean
  titulo: string
  /** O título do projecto aberto passa a ser o da aula (nome da sala ao emitir). */
  onTitulo: (v: string) => void
  aGuardar: boolean
  guardado: string
  onGuardar: (blob: Blob, duracao: number, nome: string) => Promise<void>
}) {
  const { t, i18n } = useTranslation()
  const { org } = useShell()
  const pr = useProjecto()
  const p = pr.projecto
  const leitor = useLeitor(p, pr.urls)
  const [seleccao, setSeleccao] = useState<string | null>(null)
  const [ferramenta, setFerramenta] = useState<Ferramenta>('seleccionar')
  const [ripple, setRipple] = useState(true)
  const [abaBin, setAbaBin] = useState<AbaDoBin>('fontes')
  const [abaInspector, setAbaInspector] = useState<AbaDoInspector>('corte')
  const [erro, setErro] = useState('')
  const [pausas, setPausas] = useState<{ resumo: ResumoDePausas; intervalos: Intervalo[] } | null>(null)
  const [aProcurar, setAProcurar] = useState(false)
  const [ondas, setOndas] = useState<Map<string, Float32Array>>(new Map())
  const [capitulo, setCapitulo] = useState<string | null>(null)
  const [projectos, setProjectos] = useState(false)
  const [biblioteca, setBiblioteca] = useState(false)
  const [aAbrirGravacao, setAAbrirGravacao] = useState(false)
  const agora = useAgora(1000)
  const marcaDeAgua = org?.name || getAppName()
  const ultimoResultado = useRef<Gravado | null>(null)

  // Gravação nova → projecto novo com as faixas como fontes separadas.
  useEffect(() => {
    if (!resultado || ultimoResultado.current === resultado) return
    ultimoResultado.current = resultado
    const f = resultado.faixas
    const nome = titulo.trim() || t('studio.semTitulo')
    const brutas: FonteEmBruto[] = [{ blob: f.completo, nome: t('editor.bin.nomes.completo'), origem: 'completo' }]
    if (f.video) brutas.push({ blob: f.video, nome: t('editor.bin.nomes.video'), origem: 'video', tipo: 'video' })
    if (f.audio) brutas.push({ blob: f.audio, nome: t('editor.bin.nomes.audio'), origem: 'audio', tipo: 'audio' })
    if (f.camara) brutas.push({ blob: f.camara, nome: t('editor.bin.nomes.camara'), origem: 'camara', tipo: 'video' })
    if (f.ecra) brutas.push({ blob: f.ecra, nome: t('editor.bin.nomes.ecra'), origem: 'ecra', tipo: 'video' })
    setPausas(null)
    setSeleccao(null)
    pr.criar(nome, brutas).catch((e) => setErro(apiErrorMessage(e, t('editor.erros.criar'))))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resultado])

  const abrirGravacao = useCallback(
    async (r: Pick<RecordingItem, 'id' | 'filename'>) => {
      setErro('')
      setAAbrirGravacao(true)
      setSeleccao(null)
      setPausas(null)
      try {
        const ok = await pr.abrirGravacao({ id: r.id, titulo: tituloDaGravacao(r.filename) }, async () => {
          const url = await recordingObjectUrl({ id: r.id } as RecordingItem)
          try {
            return await fetch(url).then((x) => x.blob())
          } finally {
            URL.revokeObjectURL(url)
          }
        }, r.filename)
        if (!ok) setErro(t('editor.biblioteca.erro'))
      } catch (e) {
        setErro(apiErrorMessage(e, t('editor.biblioteca.erro')))
      } finally {
        setAAbrirGravacao(false)
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [pr.abrirGravacao, t],
  )

  // Chegou com `?gravacao=<id>` (ex.: «Editar no Estúdio»): abre-a quando o
  // último projecto deste browser acabar de carregar, e tira o parâmetro do
  // endereço para um recarregar não voltar a puxá-la por cima do que se abriu depois.
  const pedidoDoEndereco = useRef(gravacaoDoEndereco())
  useEffect(() => {
    const id = pedidoDoEndereco.current
    if (!id || pr.aCarregar) return
    pedidoDoEndereco.current = null
    semGravacaoNoEndereco()
    if (p?.fontes.some((f) => f.gravacao === id)) return
    const ctl = new AbortController()
    recordingsLibrary(ctl.signal)
      .then((lista) => {
        const r = lista.find((x) => x.id === id)
        if (!r) setErro(t('editor.biblioteca.naoEncontrada'))
        else void abrirGravacao(r)
      })
      .catch((e) => {
        if (!isAbort(e)) setErro(apiErrorMessage(e, t('editor.biblioteca.erro')))
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pr.aCarregar])

  // Projecto aberto: o primeiro clipe de V1 fica seleccionado, para o
  // inspector mostrar logo a entrada/saída em vez de um cartão vazio.
  const projectoId = p?.id
  useEffect(() => {
    if (!p) return
    setSeleccao(clipsDaFaixa(p, 'V1')[0]?.id ?? null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectoId])

  const tituloDoProjecto = p?.titulo
  useEffect(() => {
    if (tituloDoProjecto !== undefined) onTitulo(tituloDoProjecto)
  }, [tituloDoProjecto, onTitulo])

  // Ondas sonoras das fontes com som (uma vez por fonte).
  useEffect(() => {
    if (!p) return
    let vivo = true
    for (const f of p.fontes) {
      if (f.tipo === 'video' || ondas.has(f.id) || !pr.urls.has(f.id)) continue
      pr
        .lerBlob(f.id)
        .then((b) => ondaDe(b))
        .then((o) => {
          if (vivo) setOndas((m) => new Map(m).set(f.id, o))
        })
        .catch(() => undefined)
    }
    return () => {
      vivo = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p?.fontes, pr.urls])

  // Uma edição invalida a análise de pausas feita antes dela.
  const clipsRef = useRef(p?.clips)
  useEffect(() => {
    if (clipsRef.current !== p?.clips) setPausas(null)
    clipsRef.current = p?.clips
  }, [p?.clips])

  const preenchimento = useMemo(() => {
    if (!p?.legendas) return null
    const ws = palavrasDasCues(p.legendas.cues).map((x) => x.palavra)
    return contarPreenchimento(encontrarPreenchimento(ws, p.legendas.lingua))
  }, [p?.legendas])

  const procurarPausas = useCallback(async () => {
    if (!p) return
    setAProcurar(true)
    setErro('')
    try {
      const fontesA1 = [...new Set(clipsDaFaixa(p, 'A1').map((c) => c.fonteId))]
      let intervalos: Intervalo[] = []
      for (const id of fontesA1) {
        const a = await analisarPausas(await pr.lerBlob(id))
        intervalos = intervalos.concat(intervalosDaFonteNaLinha(p, id, 'A1', a.pausas))
      }
      intervalos = normalizarIntervalos(intervalos).filter((i) => i.fim - i.inicio > 0.1)
      setPausas({ intervalos, resumo: { n: intervalos.length, poupanca: intervalos.reduce((n, i) => n + i.fim - i.inicio, 0) } })
    } catch (e) {
      setErro(apiErrorMessage(e, t('studio.erros.analise')))
    } finally {
      setAProcurar(false)
    }
  }, [p, pr, t])

  function teclas(e: ReactKeyboardEvent<HTMLDivElement>) {
    if (!p) return
    const alvo = e.target as HTMLElement
    if (alvo.closest('input, textarea, select, [contenteditable="true"]')) return
    const mod = e.ctrlKey || e.metaKey
    if (mod && e.key.toLowerCase() === 'z') {
      e.preventDefault()
      if (e.shiftKey) pr.refazer()
      else pr.desfazer()
    } else if (mod && e.key.toLowerCase() === 'y') {
      e.preventDefault()
      pr.refazer()
    } else if (e.key === ' ' && !alvo.closest('button')) {
      e.preventDefault()
      leitor.tocar(!leitor.aTocar)
    } else if (!mod && e.key.toLowerCase() === 's' && vista === 'edicao') {
      pr.aplicar({ tipo: 'dividir', t: leitor.tempoRef.current, ...(seleccao ? { clipIds: [seleccao] } : {}) })
    } else if (!mod && e.key.toLowerCase() === 'm' && vista === 'edicao') {
      pr.aplicar({ tipo: 'marcador', marcador: { id: novoId('m'), t: leitor.tempoRef.current, rotulo: relogio(leitor.tempoRef.current), tipo: 'marcador' } })
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      if (alvo.closest('button')) return
      e.preventDefault()
      leitor.buscar(leitor.tempoRef.current + (e.key === 'ArrowLeft' ? -1 : 1) / p.fps)
    }
  }

  const guardadoHa = pr.gravacao.guardadoEm ? Math.max(0, Math.round((agora - pr.gravacao.guardadoEm) / 1000)) : null

  const topoMarca = (
    <button type="button" className="ed-top__brand" onClick={() => onVista('emissao')} title={t('editor.topo.voltarEmissao')}>
      <DelonixSymbol size={22} />
      <span className="ed-top__studio">{t('editor.topo.studio')}</span>
    </button>
  )

  const separadores = (
    <nav className="ed-top__tabs" aria-label={t('editor.topo.vistas')}>
      {(['edicao', 'legendas', 'exportacoes'] as const).map((v) => (
        <button key={v} type="button" aria-current={vista === v ? 'page' : undefined} data-studio-vista={v} onClick={() => onVista(v)}>
          {t(`editor.topo.${v}`)}
        </button>
      ))}
    </nav>
  )

  const dialogos = (
    <>
      {biblioteca && <Biblioteca onFechar={() => setBiblioteca(false)} onEscolher={abrirGravacao} />}
      {capitulo !== null && p && (
        <Dialog
          title={t('editor.linha.capitulo')}
          onClose={() => setCapitulo(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setCapitulo(null)}>
                {t('editor.legendas.cancelar')}
              </Button>
              <Button
                variant="primary"
                disabled={!capitulo.trim()}
                onClick={() => {
                  pr.aplicar({ tipo: 'marcador', marcador: { id: novoId('m'), t: leitor.tempoRef.current, rotulo: capitulo.trim(), tipo: 'capitulo' } })
                  setCapitulo(null)
                }}
              >
                {t('editor.linha.criarCapitulo')}
              </Button>
            </>
          }
        >
          <TextInput value={capitulo} maxLength={80} aria-label={t('editor.linha.nomeCapitulo')} placeholder={t('editor.linha.nomeCapitulo')} onChange={(e) => setCapitulo(e.target.value)} />
          <p className="st-note">{t('editor.linha.capituloNota', { t: relogio(leitor.tempoRef.current) })}</p>
        </Dialog>
      )}
      {projectos && (
        <Dialog title={t('editor.projectos.titulo')} onClose={() => setProjectos(false)} wide>
          <p className="st-note">{t('editor.projectos.nota')}</p>
          <ul className="ed-lib">
            {pr.lista.map((r) => (
              <li key={r.id} className="ed-lib__row">
                <button
                  type="button"
                  className={cx('ed-lib__item', r.id === p?.id && 'ed-lib__item--on')}
                  onClick={() => {
                    void pr.guardarJa().then(() => pr.abrir(r.id))
                    setSeleccao(null)
                    setProjectos(false)
                  }}
                >
                  <span className="ed-lib__name">{r.projecto.titulo}</span>
                  <span className="dx-num dx-muted">
                    {r.id.slice(-6)} · {relogio(duracaoDoProjecto(r.projecto))} · {new Date(r.alteradoEm).toLocaleString(i18n.language)}
                  </span>
                </button>
                <IconButton icon="trash" bare label={t('editor.projectos.apagar', { titulo: r.projecto.titulo })} onClick={() => void pr.apagar(r.id)} />
              </li>
            ))}
          </ul>
          <Button
            variant="ghost"
            icon="film"
            onClick={() => {
              setProjectos(false)
              setBiblioteca(true)
            }}
          >
            {t('editor.biblioteca.abrir')}
          </Button>
          <Button
            variant="secondary"
            icon="plus"
            onClick={() => {
              setProjectos(false)
              void pr.criar(t('editor.projectos.novoTitulo'), [])
            }}
          >
            {t('editor.projectos.novo')}
          </Button>
        </Dialog>
      )}
    </>
  )

  if (!p) {
    return (
      <div className="dx-stage ed ed--vazio">
        <header className="ed-top">
          {topoMarca}
          <span className="ed-top__sep" aria-hidden="true" />
          {separadores}
        </header>
        <div className="ed-empty">
          {pr.aCarregar || aAbrirGravacao ? (
            <p className="st-note">{t('editor.aCarregar')}</p>
          ) : (
            <Empty
              icon="film"
              title={t('studio.edicao.take')}
              action={
                <div className="st-actions st-actions--center">
                  <Button variant="secondary" icon="record" onClick={() => onVista('emissao')}>
                    {t('editor.vazio.gravar')}
                  </Button>
                  <Button variant="secondary" icon="film" onClick={() => setBiblioteca(true)} data-studio="abrir-gravacao">
                    {t('editor.biblioteca.abrir')}
                  </Button>
                  <Button variant="primary" icon="plus" onClick={() => void pr.criar(t('editor.projectos.novoTitulo'), [])} data-studio="novo-projecto">
                    {t('editor.projectos.novo')}
                  </Button>
                  {pr.lista.length > 0 && (
                    <Button variant="ghost" onClick={() => setProjectos(true)}>
                      {t('editor.projectos.abrir')}
                    </Button>
                  )}
                </div>
              }
            >
              {t('editor.vazio.texto')}
            </Empty>
          )}
          {erro && <Alert tone="danger">{erro}</Alert>}
        </div>
        {dialogos}
      </div>
    )
  }

  if (vista === 'exportacoes') {
    return (
      <div className="ed-page">
        <ExportsPanel
          projecto={p}
          lista={pr.lista}
          lerBlob={pr.lerBlob}
          marcaDeAgua={marcaDeAgua}
          aGuardar={aGuardar}
          guardado={guardado}
          onGuardar={onGuardar}
          antesDeExportar={pr.guardarJa}
          onEditor={() => onVista('edicao')}
        />
      </div>
    )
  }

  return (
    <div className="dx-stage ed" onKeyDown={teclas}>
      {vista === 'edicao' ? (
        <header className="ed-top">
          {topoMarca}
          <span className="ed-top__sep" aria-hidden="true" />
          <input
            className="ed-top__title"
            value={p.titulo}
            maxLength={80}
            aria-label={t('studio.edicao.destino.campoTitulo')}
            onChange={(e) => pr.aplicar({ tipo: 'titulo', titulo: e.target.value }, 'titulo')}
          />
          <button type="button" className="ed-top__meta dx-num" onClick={() => setProjectos(true)} title={t('editor.projectos.titulo')}>
            {p.altura}p · {relogio(duracaoDoProjecto(p))} · {t('editor.topo.projecto', { id: p.id.slice(-6) })}
          </button>
          <span className="dx-spacer" />
          <span className={cx('ed-top__saved dx-num', pr.gravacao.erro && 'ed-top__saved--erro')} role="status" data-studio="guardado-auto">
            {pr.gravacao.aGuardar
              ? t('editor.topo.aGuardar')
              : pr.gravacao.erro
                ? t('editor.topo.naoGuardado')
                : guardadoHa !== null
                  ? t('editor.topo.guardadoHa', { s: guardadoHa })
                  : ''}
          </span>
          <button type="button" className="ed-top__btn" disabled={!pr.podeDesfazer} onClick={pr.desfazer} data-studio="desfazer">
            {t('editor.topo.desfazer')}
          </button>
          <IconButton icon="repeat" className="ed-top__icon" label={t('editor.topo.refazer')} disabled={!pr.podeRefazer} onClick={pr.refazer} data-studio="refazer" />
          <button type="button" className="ed-top__btn" onClick={() => leitor.tocar(!leitor.aTocar)} aria-pressed={leitor.aTocar}>
            {leitor.aTocar ? t('editor.transporte.pausa') : t('editor.topo.previsualizar')}
          </button>
          <button type="button" className="ed-top__btn ed-top__btn--primary" onClick={() => onVista('exportacoes')} data-studio="ir-exportar">
            {t('editor.topo.exportar')}
          </button>
        </header>
      ) : (
        <header className="ed-top">
          {topoMarca}
          <span className="ed-top__sep" aria-hidden="true" />
          {separadores}
          <span className="dx-spacer" />
          <span className="ed-top__local dx-num">{t('editor.topo.modeloLocal')}</span>
        </header>
      )}

      <div className="ed-notices">
        {!podeCortar ? (
          <Alert tone="warning">{t('studio.edicao.semWebCodecs')}</Alert>
        ) : null}
        {erro && <Alert tone="danger">{erro}</Alert>}
        {pr.gravacao.erro && <Alert tone="warning">{t('editor.erros.autoGuardar')}</Alert>}
      </div>

      {vista === 'edicao' ? (
        <>
          <div className="ed-body">
            <Bin
              p={p}
              aba={abaBin}
              onAba={setAbaBin}
              aplicar={pr.aplicar}
              acrescentar={pr.acrescentar}
              onErro={setErro}
              pausas={pausas?.resumo ?? null}
              aProcurarPausas={aProcurar}
              onProcurarPausas={() => void procurarPausas()}
              onAplicarPausas={() => {
                if (pausas) pr.aplicar({ tipo: 'cortar-intervalos', intervalos: pausas.intervalos })
                setPausas(null)
              }}
              onCancelarPausas={() => setPausas(null)}
              preenchimento={preenchimento}
              onIrParaLegendas={() => onVista('legendas')}
              marcaDeAgua={marcaDeAgua}
            />
            <section className="ed-centre" aria-label={t('studio.palco.previsualizacao')}>
              <Preview projecto={p} leitor={leitor} lingua={p.legendas?.lingua ?? null} marcaDeAgua={marcaDeAgua} forma="edicao" onLegendas={() => onVista('legendas')} />
              <div className="ed-tools" role="toolbar" aria-label={t('studio.edicao.ferramentas')}>
                {FERRAMENTAS.map((f) => (
                  <button key={f.id} type="button" className="ed-tool" aria-pressed={ferramenta === f.id} onClick={() => setFerramenta(f.id)} data-ferramenta={f.id}>
                    <span className="ed-tool__icon" aria-hidden="true">
                      <Icon name={f.icone} size={13} />
                    </span>
                    {t(`editor.ferramentas.${f.id}`)}
                  </button>
                ))}
                <span className="dx-spacer" />
                <label className={cx('ed-ripple', ripple && 'ed-ripple--on')}>
                  <input type="checkbox" role="switch" checked={ripple} onChange={(e) => setRipple(e.target.checked)} />
                  {t('editor.ferramentas.ripple')}
                </label>
              </div>
            </section>
            <Inspector p={p} seleccao={seleccao} aba={abaInspector} onAba={setAbaInspector} aplicar={pr.aplicar} leitor={leitor} ripple={ripple} ferramenta={ferramenta} />
          </div>
          <Timeline
            projecto={p}
            leitor={leitor}
            seleccao={seleccao}
            onSeleccionar={setSeleccao}
            ferramenta={ferramenta}
            ripple={ripple}
            aplicar={pr.aplicar}
            ondas={ondas}
            onCapitulo={() => setCapitulo('')}
            onFerramentaUsada={(f) => {
              if (f === 'transicao' || f === 'mascara') setAbaInspector('corte')
              if (f === 'audio') setAbaInspector('audio')
              if (f === 'texto') setAbaInspector('texto')
            }}
          />
        </>
      ) : (
        <CaptionsPanel projecto={p} leitor={leitor} aplicar={pr.aplicar} lerBlob={pr.lerBlob} onErro={setErro} marcaDeAgua={marcaDeAgua} />
      )}
      {dialogos}
    </div>
  )
}
