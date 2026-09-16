/**
 * «IA no servidor» — o LLM local da organização (Ollama, atrás do servidor)
 * aplicado à transcrição do projecto: resumo e capítulos, título/descrição/
 * etiquetas para publicar, e palavras de preenchimento desta transcrição.
 *
 * O browser NUNCA fala com o Ollama: pede ao servidor, que verifica a pertença
 * à organização, limita os pedidos simultâneos por organização e não guarda
 * nada. O que aqui aparece é SUGESTÃO — nada se aplica sem um clique, e cada
 * acção usa um caminho que já existe (edição do projecto, PATCH da gravação).
 *
 * Quando o modelo não está disponível o cartão diz porquê (a razão vem do
 * servidor) e não mostra botões que não fariam nada.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorMessage, isAbort, studioAi, studioAiStatus, updateRecording } from '../../api'
import type { StudioAiFillers, StudioAiPublication, StudioAiStatus, StudioAiSummary, StudioAiTask } from '../../api'
import { useShell } from '../../components/shellContext'
import { Button, Dialog, TextArea, TextInput } from '../../ui/kit'
import { contarPreenchimento, encontrarPreenchimento, palavrasDasCues, relogio } from '../captions/legendas'
import { fonteDaBiblioteca, segmentosDasCues } from '../captions/servidor'
import type { Edicao, Projecto } from './projecto'
import { capitulos, novoId } from './projecto'

type Resultado =
  | { tarefa: 'summary'; dados: StudioAiSummary }
  | { tarefa: 'publication'; dados: StudioAiPublication }
  | { tarefa: 'fillers'; dados: StudioAiFillers }

type Estado = { fase: 'a-ver' } | { fase: 'pronto'; s: StudioAiStatus } | { fase: 'erro'; msg: string }

const TAREFAS: StudioAiTask[] = ['summary', 'publication', 'fillers']

export default function AssistenteIA({
  p,
  aplicar,
  onIrParaLegendas,
  onTermos,
}: {
  p: Projecto
  aplicar: (e: Edicao, chave?: string | null) => void
  onIrParaLegendas: () => void
  /** Os termos de preenchimento encontrados passam para as Legendas. */
  onTermos: (termos: string[]) => void
}) {
  const { t } = useTranslation()
  const { org } = useShell()
  const [estado, setEstado] = useState<Estado>({ fase: 'a-ver' })
  const [aCorrer, setACorrer] = useState<StudioAiTask | null>(null)
  const [erro, setErro] = useState('')
  const [resultado, setResultado] = useState<Resultado | null>(null)
  const pedido = useRef<AbortController | null>(null)

  const orgId = org?.id
  useEffect(() => {
    if (!orgId) return
    const ctl = new AbortController()
    setEstado({ fase: 'a-ver' })
    studioAiStatus(orgId, ctl.signal)
      .then((s) => setEstado({ fase: 'pronto', s }))
      .catch((e) => {
        if (!isAbort(e)) setEstado({ fase: 'erro', msg: apiErrorMessage(e, t('editor.iaServidor.estadoFalhou')) })
      })
    return () => {
      ctl.abort()
      pedido.current?.abort()
    }
  }, [orgId, t])

  const cues = p.legendas?.cues ?? []
  const temTranscricao = cues.some((c) => c.texto.trim())
  const s = estado.fase === 'pronto' ? estado.s : null
  const disponivel = !!s && s.configured && s.reachable && s.model_installed !== false

  async function pedir(tarefa: StudioAiTask) {
    if (!orgId || !p.legendas) return
    pedido.current?.abort()
    const ctl = new AbortController()
    pedido.current = ctl
    setACorrer(tarefa)
    setErro('')
    try {
      const corpo = { task: tarefa, language: p.legendas.lingua, title: p.titulo.slice(0, 200), segments: segmentosDasCues(p.legendas.cues) }
      if (tarefa === 'summary') setResultado({ tarefa, dados: await studioAi<StudioAiSummary>(orgId, corpo, ctl.signal) })
      else if (tarefa === 'publication') setResultado({ tarefa, dados: await studioAi<StudioAiPublication>(orgId, corpo, ctl.signal) })
      else setResultado({ tarefa, dados: await studioAi<StudioAiFillers>(orgId, corpo, ctl.signal) })
    } catch (e) {
      if (isAbort(e)) return
      setErro(e instanceof ApiError && e.status === 429 ? t('editor.iaServidor.ocupado') : apiErrorMessage(e, t('editor.iaServidor.falhou')))
    } finally {
      if (pedido.current === ctl) {
        pedido.current = null
        setACorrer(null)
      }
    }
  }

  const linhaDeEstado =
    estado.fase === 'a-ver'
      ? t('editor.iaServidor.aVer')
      : estado.fase === 'erro'
        ? estado.msg
        : !estado.s.configured || !estado.s.reachable || estado.s.model_installed === false
          ? estado.s.error ?? t('editor.iaServidor.indisponivel')
          : t('editor.iaServidor.modelo', { modelo: estado.s.model })

  return (
    <section className="ed-group" data-studio="ia-servidor">
      <h2 className="ed-label">{t('editor.iaServidor.titulo')}</h2>
      <div className="ed-card">
        <span className="ed-card__title">{t('editor.iaServidor.assistente')}</span>
        <span className={disponivel || estado.fase === 'a-ver' ? 'dx-num ed-src__sub' : 'dx-num ed-src__sub ed-tone--erro'} role="status" data-studio="ia-estado">
          {linhaDeEstado}
        </span>
        {disponivel && !temTranscricao && (
          <button type="button" className="ed-link" onClick={onIrParaLegendas}>
            {t('editor.iaServidor.precisaTranscricao')}
          </button>
        )}
        {disponivel &&
          temTranscricao &&
          TAREFAS.map((tarefa) => (
            <button
              key={tarefa}
              type="button"
              className="ed-btn"
              disabled={aCorrer !== null}
              onClick={() => void pedir(tarefa)}
              data-studio={`ia-${tarefa}`}
            >
              {aCorrer === tarefa ? t('editor.iaServidor.aPensar') : t(`editor.iaServidor.tarefas.${tarefa}`)}
            </button>
          ))}
        {aCorrer && (
          <button type="button" className="ed-link" onClick={() => pedido.current?.abort()}>
            {t('editor.legendas.cancelar')}
          </button>
        )}
        {erro && (
          <span className="ed-src__sub ed-tone--erro" role="alert" data-studio="ia-erro">
            {erro}
          </span>
        )}
      </div>
      {resultado && (
        <DialogoDoResultado
          p={p}
          r={resultado}
          aplicar={aplicar}
          onFechar={() => setResultado(null)}
          onRever={(termos) => {
            onTermos(termos)
            setResultado(null)
            onIrParaLegendas()
          }}
        />
      )}
    </section>
  )
}

function DialogoDoResultado({
  p,
  r,
  aplicar,
  onFechar,
  onRever,
}: {
  p: Projecto
  r: Resultado
  aplicar: (e: Edicao, chave?: string | null) => void
  onFechar: () => void
  onRever: (termos: string[]) => void
}) {
  const { t } = useTranslation()
  const [aviso, setAviso] = useState('')
  const [aGuardar, setAGuardar] = useState(false)
  const [titulo, setTitulo] = useState(r.tarefa === 'publication' ? r.dados.title : '')
  const [descricao, setDescricao] = useState(r.tarefa === 'publication' ? r.dados.description : '')
  const [etiquetas, setEtiquetas] = useState(r.tarefa === 'publication' ? r.dados.tags.join(', ') : '')
  const daBiblioteca = fonteDaBiblioteca(p)

  const ocorrencias = useMemo(() => {
    if (r.tarefa !== 'fillers' || !p.legendas) return []
    const ws = palavrasDasCues(p.legendas.cues).map((x) => x.palavra)
    // Só os termos do LLM: a lista fixa já aparece no cartão ao lado.
    return contarPreenchimento(encontrarPreenchimento(ws, 'sem-lista-fixa', r.dados.terms))
  }, [r, p.legendas])

  async function copiar(texto: string) {
    try {
      await navigator.clipboard.writeText(texto)
      setAviso(t('editor.iaServidor.copiado'))
    } catch {
      setAviso(t('editor.iaServidor.copiaFalhou'))
    }
  }

  if (r.tarefa === 'summary') {
    const existentes = new Set(capitulos(p).map((m) => Math.round(m.t)))
    const novos = r.dados.chapters.filter((c) => !existentes.has(Math.round(c.t_ms / 1000)))
    return (
      <Dialog
        title={t('editor.iaServidor.tarefas.summary')}
        onClose={onFechar}
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => void copiar(r.dados.summary)}>
              {t('editor.iaServidor.copiarResumo')}
            </Button>
            <Button
              variant="primary"
              disabled={!novos.length}
              data-studio="ia-criar-capitulos"
              onClick={() => {
                for (const c of novos) aplicar({ tipo: 'marcador', marcador: { id: novoId('m'), t: c.t_ms / 1000, rotulo: c.title, tipo: 'capitulo' } })
                onFechar()
              }}
            >
              {t('editor.iaServidor.criarCapitulos', { count: novos.length })}
            </Button>
          </>
        }
      >
        <p className="ed-text" data-studio="ia-resumo">
          {r.dados.summary}
        </p>
        {r.dados.chapters.length ? (
          <ol className="ed-ai-list">
            {r.dados.chapters.map((c) => (
              <li key={`${c.t_ms}-${c.title}`}>
                <span className="dx-num dx-muted">{relogio(c.t_ms / 1000)}</span> {c.title}
              </li>
            ))}
          </ol>
        ) : (
          <p className="st-note">{t('editor.iaServidor.semCapitulos')}</p>
        )}
        <p className="st-note">{t('editor.iaServidor.sugestao')}</p>
        {aviso && <p className="st-note">{aviso}</p>}
      </Dialog>
    )
  }

  if (r.tarefa === 'publication') {
    const tags = etiquetas
      .split(',')
      .map((x) => x.trim())
      .filter(Boolean)
    return (
      <Dialog
        title={t('editor.iaServidor.tarefas.publication')}
        onClose={onFechar}
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => void copiar([titulo, descricao, tags.map((x) => `#${x}`).join(' ')].filter(Boolean).join('\n\n'))}>
              {t('editor.iaServidor.copiar')}
            </Button>
            <Button variant="secondary" disabled={!titulo.trim()} onClick={() => aplicar({ tipo: 'titulo', titulo: titulo.trim().slice(0, 80) })} data-studio="ia-usar-titulo">
              {t('editor.iaServidor.usarTitulo')}
            </Button>
            {daBiblioteca && (
              <Button
                variant="primary"
                busy={aGuardar}
                data-studio="ia-guardar-gravacao"
                onClick={async () => {
                  setAGuardar(true)
                  setAviso('')
                  try {
                    await updateRecording(daBiblioteca.gravacao, { description: descricao.trim(), tags })
                    setAviso(t('editor.iaServidor.guardadoNaGravacao'))
                  } catch (e) {
                    setAviso(apiErrorMessage(e, t('editor.iaServidor.guardarFalhou')))
                  } finally {
                    setAGuardar(false)
                  }
                }}
              >
                {t('editor.iaServidor.guardarNaGravacao')}
              </Button>
            )}
          </>
        }
      >
        <label className="ed-slider__label" htmlFor="ed-ia-titulo">
          {t('editor.iaServidor.campoTitulo')}
        </label>
        <TextInput id="ed-ia-titulo" value={titulo} maxLength={100} onChange={(e) => setTitulo(e.target.value)} data-studio="ia-titulo" />
        <label className="ed-slider__label" htmlFor="ed-ia-descricao">
          {t('editor.iaServidor.campoDescricao')}
        </label>
        <TextArea id="ed-ia-descricao" value={descricao} rows={5} maxLength={1000} onChange={(e) => setDescricao(e.target.value)} />
        <label className="ed-slider__label" htmlFor="ed-ia-etiquetas">
          {t('editor.iaServidor.campoEtiquetas')}
        </label>
        <TextInput id="ed-ia-etiquetas" value={etiquetas} onChange={(e) => setEtiquetas(e.target.value)} />
        <p className="st-note">{daBiblioteca ? t('editor.iaServidor.notaBiblioteca') : t('editor.iaServidor.notaLocal')}</p>
        {aviso && (
          <p className="st-note" role="status" data-studio="ia-aviso">
            {aviso}
          </p>
        )}
      </Dialog>
    )
  }

  const termos = ocorrencias.map((o) => o.termo)
  return (
    <Dialog
      title={t('editor.iaServidor.tarefas.fillers')}
      onClose={onFechar}
      footer={
        <Button variant="primary" disabled={!termos.length} onClick={() => onRever(termos)} data-studio="ia-rever-preenchimento">
          {t('editor.iaServidor.rever')}
        </Button>
      }
    >
      {ocorrencias.length ? (
        <ul className="ed-ai-list" data-studio="ia-termos">
          {ocorrencias.map((o) => (
            <li key={o.termo}>{t('editor.ia.ocorrencia', { n: o.n, termo: o.termo })}</li>
          ))}
        </ul>
      ) : (
        <p className="st-note">{t('editor.iaServidor.semTermos')}</p>
      )}
      <p className="st-note">{t('editor.iaServidor.notaPreenchimento')}</p>
    </Dialog>
  )
}
