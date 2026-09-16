/**
 * «Exportações», na grelha do template: barra da página com Fila/Histórico/
 * Predefinições; à esquerda o que está a processar e o histórico; à direita a
 * nova exportação.
 *
 * Tudo corre NESTE browser: predefinições que o WebCodecs cumpre, legendas
 * queimadas, marca de água, estimativa, uma fila local (uma de cada vez, que
 * se pode pausar e reordenar) e o histórico deste dispositivo.
 *
 * O que NÃO está, porque é do servidor e o servidor ainda não o tem: a fila de
 * transcodificação partilhada e o seu nó/CPU, o 4K/H.265, as faixas de
 * dobragem, o quadro como capítulo, o destino MinIO, «descarregar todos» do
 * histórico do servidor e a publicação automática no YouTube/Odoo.
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort } from '../../api'
import PageBar from '../../components/PageBar'
import { Alert, Button, Checkbox, cx, Select } from '../../ui/kit'
import { relogio } from '../captions/legendas'
import * as bd from '../edit/bd'
import type { Projecto } from '../edit/projecto'
import { duracaoDoProjecto } from '../edit/projecto'
import { exportacaoSuportada, renderizar } from '../edit/render'
import type { ProgressoDeRender } from '../edit/render'
import { nomeDeFicheiro, predefinicao, PREDEFINICOES, tamanhoEstimado, tamanhoLegivel, tempoEstimado } from './predefinicoes'
import type { IdDaPredefinicao } from './predefinicoes'

const FPS_MEDIDOS = 'dx_editor_fps'

type Destino = 'descarregar' | 'biblioteca'
type Aba = 'fila' | 'historico' | 'predefinicoes'

interface Trabalho {
  id: string
  projecto: Projecto
  preset: IdDaPredefinicao
  legendas: string | null
  marcaDeAgua: boolean
  destino: Destino
  estado: 'espera' | 'a-exportar' | 'concluida' | 'falhou' | 'cancelada'
  progresso: ProgressoDeRender | null
  erro?: string
  resultado?: { url: string; bytes: number; duracao: number; nome: string; audio: boolean }
}

function lerFps(): number | null {
  try {
    const v = Number(localStorage.getItem(FPS_MEDIDOS))
    return Number.isFinite(v) && v > 0 ? v : null
  } catch {
    return null
  }
}

function lerToken(nome: string, omisso: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(nome).trim()
  return v || omisso
}

/** Passos da exportação no browser, pela ordem em que correm. */
function passos(j: Trabalho): { id: string; estado: 'ok' | 'run' | 'wait' }[] {
  const p = predefinicao(j.preset)
  const fase = j.progresso?.fase
  const feito = j.estado === 'concluida'
  const lista = ['audio', ...(p.soAudio ? [] : ['video']), ...(j.legendas ? ['legendas'] : []), j.destino === 'biblioteca' ? 'biblioteca' : 'ficheiro']
  const ordem = (x: string) => (x === 'audio' ? 0 : x === 'video' || x === 'legendas' ? 1 : 2)
  const actual = fase === 'audio' ? 0 : fase === 'video' ? 1 : fase === 'a-fechar' ? 2 : -1
  return lista.map((id) => ({
    id,
    estado: feito || (actual >= 0 && ordem(id) < actual) ? 'ok' : actual === ordem(id) ? 'run' : 'wait',
  }))
}

export default function ExportsPanel({
  projecto,
  lista,
  lerBlob,
  marcaDeAgua,
  aGuardar,
  guardado,
  onGuardar,
  antesDeExportar,
  onEditor,
}: {
  projecto: Projecto
  lista: bd.RegistoDeProjecto[]
  lerBlob: (id: string) => Promise<Blob>
  marcaDeAgua: string
  aGuardar: boolean
  guardado: string
  onGuardar: (blob: Blob, duracao: number, nome: string) => Promise<void>
  antesDeExportar: () => Promise<void>
  onEditor: () => void
}) {
  const { t, i18n } = useTranslation()
  const [aba, setAba] = useState<Aba>('fila')
  const [origemId, setOrigemId] = useState(projecto.id)
  const [preset, setPreset] = useState<IdDaPredefinicao>('web1080')
  const [legendas, setLegendas] = useState<string>('')
  const [comMarca, setComMarca] = useState(false)
  const [destino, setDestino] = useState<Destino>('descarregar')
  const [fila, setFila] = useState<Trabalho[]>([])
  const [pausada, setPausada] = useState(false)
  const [historico, setHistorico] = useState<bd.RegistoDeExportacao[]>([])
  const [fps, setFps] = useState<number | null>(lerFps)
  const controlo = useRef<AbortController | null>(null)
  const aCorrer = useRef(false)
  const formulario = useRef<HTMLDivElement>(null)

  useEffect(() => setOrigemId(projecto.id), [projecto.id])

  const recarregarHistorico = useCallback(() => {
    bd.listarExportacoes()
      .then(setHistorico)
      .catch(() => setHistorico([]))
  }, [])
  useEffect(() => recarregarHistorico(), [recarregarHistorico])

  const filaRef = useRef(fila)
  filaRef.current = fila
  useEffect(
    () => () => {
      controlo.current?.abort()
      for (const j of filaRef.current) if (j.resultado) URL.revokeObjectURL(j.resultado.url)
    },
    [],
  )

  const origem = origemId === projecto.id ? projecto : lista.find((r) => r.id === origemId)?.projecto ?? projecto
  const dur = duracaoDoProjecto(origem)
  const pr = predefinicao(preset)
  const linguas = origem.legendas ? [origem.legendas.lingua, ...Object.keys(origem.legendas.traducoes)] : []
  const suportado = exportacaoSuportada(pr.soAudio)
  const semSom = pr.soAudio && !origem.clips.some((c) => c.faixa === 'A1' || c.faixa === 'A2')

  const mudar = (id: string, patch: Partial<Trabalho>) => setFila((f) => f.map((j) => (j.id === id ? { ...j, ...patch } : j)))

  const registar = useCallback(
    (j: Trabalho, estado: bd.RegistoDeExportacao['estado'], bytes: number, duracao: number, segundos: number, erro?: string) =>
      bd
        .registarExportacao({
          id: j.id,
          projectoId: j.projecto.id,
          titulo: j.projecto.titulo,
          predefinicao: j.preset,
          formato: predefinicao(j.preset).soAudio ? 'WEBA' : 'WEBM',
          bytes,
          duracao,
          destino: j.destino === 'descarregar' ? 'descarregado' : 'biblioteca',
          estado,
          ...(erro ? { erro } : {}),
          criadaEm: Date.now(),
          segundos,
        })
        .catch(() => undefined),
    [],
  )

  const correr = useCallback(
    async (j: Trabalho) => {
      aCorrer.current = true
      const ctl = new AbortController()
      controlo.current = ctl
      mudar(j.id, { estado: 'a-exportar', progresso: { fase: 'audio', fraccao: 0 } })
      const p = predefinicao(j.preset)
      const inicio = performance.now()
      const leitor =
        j.projecto.id === projecto.id
          ? lerBlob
          : async (id: string) => {
              const b = await bd.lerFonte(id)
              if (!b) throw new Error(t('editor.exportar.fonteEmFalta'))
              return b
            }
      const nome = nomeDeFicheiro(j.projecto.titulo, j.preset, p.extensao)
      try {
        const r = await renderizar(
          { ...j.projecto, marca: { ...j.projecto.marca, marcaDeAgua: j.marcaDeAgua } },
          leitor,
          {
            largura: p.largura,
            altura: p.altura,
            fps: p.fps,
            videoBps: p.videoBps,
            audioBps: p.audioBps,
            enquadramento: p.enquadramento,
            soAudio: p.soAudio,
            legendas: j.legendas,
            marcaDeAgua: j.marcaDeAgua ? marcaDeAgua : null,
            destaque: lerToken('--live', '#f0a32e'),
            familia: lerToken('--font-ui', 'system-ui, sans-serif'),
          },
          (progresso) => mudar(j.id, { progresso }),
          ctl.signal,
        )
        if (r.fpsMedidos) {
          setFps(r.fpsMedidos)
          try {
            localStorage.setItem(FPS_MEDIDOS, String(Math.round(r.fpsMedidos)))
          } catch {
            /* sem localStorage */
          }
        }
        const url = URL.createObjectURL(r.blob)
        mudar(j.id, { estado: 'concluida', progresso: null, resultado: { url, bytes: r.blob.size, duracao: r.duracao, nome, audio: p.soAudio } })
        if (j.destino === 'descarregar') {
          const a = document.createElement('a')
          a.href = url
          a.download = nome
          a.click()
        } else {
          await onGuardar(r.blob, r.duracao, j.projecto.titulo)
        }
        await registar(j, 'concluida', r.blob.size, r.duracao, (performance.now() - inicio) / 1000)
      } catch (e) {
        const cancelada = isAbort(e)
        const msg = cancelada ? undefined : apiErrorMessage(e, t('editor.exportar.falhou'))
        mudar(j.id, { estado: cancelada ? 'cancelada' : 'falhou', progresso: null, erro: msg })
        await registar(j, cancelada ? 'cancelada' : 'falhou', 0, 0, (performance.now() - inicio) / 1000, msg)
      } finally {
        controlo.current = null
        aCorrer.current = false
        setFila((f) => [...f])
        recarregarHistorico()
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [projecto.id, lerBlob, marcaDeAgua, onGuardar, recarregarHistorico, registar, t],
  )

  // A fila anda sozinha: acabou um, começa o seguinte (a menos que esteja em pausa).
  useEffect(() => {
    if (aCorrer.current || pausada) return
    const proximo = fila.find((j) => j.estado === 'espera')
    if (proximo) void correr(proximo)
  }, [fila, correr, pausada])

  async function colocar() {
    if (origem.id === projecto.id) await antesDeExportar()
    setFila((f) => [
      ...f,
      {
        id: `e${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
        projecto: origem,
        preset,
        legendas: !pr.soAudio && legendas ? legendas : null,
        marcaDeAgua: !pr.soAudio && comMarca,
        destino,
        estado: 'espera',
        progresso: null,
      },
    ])
    setAba('fila')
  }

  const activos = fila.filter((j) => j.estado === 'a-exportar').length
  const emEspera = fila.filter((j) => j.estado === 'espera').length
  const visiveis = fila.filter((j) => j.estado === 'espera' || j.estado === 'a-exportar' || (j.estado !== 'cancelada' && j.resultado) || j.estado === 'falhou')

  const tabela = (
    <div className="ed-exp__table dx-table-wrap">
      <table className="dx-table">
        <thead>
          <tr>
            <th>{t('editor.exportar.colunas.exportacao')}</th>
            <th>{t('editor.exportar.colunas.predefinicao')}</th>
            <th>{t('editor.exportar.colunas.formato')}</th>
            <th>{t('editor.exportar.colunas.tamanho')}</th>
            <th>{t('editor.exportar.colunas.estado')}</th>
            <th>{t('editor.exportar.colunas.destino')}</th>
          </tr>
        </thead>
        <tbody>
          {!historico.length && (
            <tr>
              <td colSpan={6} className="dx-muted">
                {t('editor.exportar.historicoVazio')}
              </td>
            </tr>
          )}
          {historico.map((h) => (
            <tr key={h.id}>
              <td>
                <span className="ed-exp__title">{h.titulo}</span>
                <span className="dx-num ed-exp__sub">{new Date(h.criadaEm).toLocaleString(i18n.language)}</span>
              </td>
              <td>{t(`editor.exportar.presets.${h.predefinicao}.nome`)}</td>
              <td>
                <span className="ed-exp__fmt dx-num">{h.formato}</span>
              </td>
              <td className="dx-num">{h.bytes ? tamanhoLegivel(h.bytes, i18n.language) : '—'}</td>
              <td>
                <span className={cx('ed-exp__state', `ed-exp__state--${h.estado}`)} title={h.erro}>
                  {t(`editor.exportar.estados.${h.estado}`)}
                </span>
              </td>
              <td>{t(`editor.exportar.destinos.${h.destino}`)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )

  return (
    <>
      <PageBar
        title={t('editor.topo.exportacoes')}
        meta={
          <div className="ed-exp__tabs" role="tablist" aria-label={t('editor.topo.exportacoes')}>
            {(['fila', 'historico', 'predefinicoes'] as const).map((a) => (
              <button key={a} type="button" role="tab" aria-selected={aba === a} onClick={() => setAba(a)}>
                {t(`editor.exportar.abas.${a}`, { n: a === 'fila' ? activos + emEspera : a === 'historico' ? historico.length : PREDEFINICOES.length })}
              </button>
            ))}
          </div>
        }
      >
        <Button variant="ghost" icon="film" onClick={onEditor}>
          {t('editor.topo.edicao')}
        </Button>
        <Button variant="secondary" onClick={() => setPausada(!pausada)} aria-pressed={pausada}>
          {pausada ? t('editor.exportar.retomarFila') : t('editor.exportar.pausarFila')}
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            formulario.current?.scrollIntoView({ block: 'nearest' })
            formulario.current?.querySelector<HTMLElement>('select, input, button')?.focus()
          }}
        >
          {t('editor.exportar.nova')}
        </Button>
      </PageBar>

      <div className="ed-exp">
        <div className="ed-exp__main">
          {aba === 'fila' && (
            <>
              <div className="ed-exp__head">
                <h2>{t('editor.exportar.aProcessar')}</h2>
                <span className="dx-num ed-exp__sub">{t('editor.exportar.contagem', { activas: activos, espera: emEspera })}</span>
                {pausada && <span className="ed-exp__state ed-exp__state--cancelada">{t('editor.exportar.filaPausada')}</span>}
              </div>
              <div className="ed-exp__queue" data-studio="fila">
                {!visiveis.length && <p className="dx-muted ed-exp__empty">{t('editor.exportar.filaVazia')}</p>}
                {visiveis.map((j, i) => (
                  <div key={j.id} className={cx('ed-job', j.estado === 'a-exportar' && 'ed-job--on')} data-estado={j.estado}>
                    <div className="ed-job__row">
                      <div className="ed-job__meta">
                        <span className="ed-exp__title">{j.projecto.titulo}</span>
                        <span className="dx-num ed-exp__sub">
                          {t(`editor.exportar.presets.${j.preset}.spec`)}
                          {j.legendas ? ` · ${t('editor.exportar.comLegendas', { l: j.legendas })}` : ''}
                        </span>
                      </div>
                      <div className="ed-job__state">
                        <span className={cx('dx-num ed-job__label', `ed-exp__state--${j.estado}`)}>{t(`editor.exportar.estados.${j.estado === 'a-exportar' ? 'aExportar' : j.estado}`)}</span>
                        <span className="dx-num ed-exp__sub">
                          {j.estado === 'espera'
                            ? t('editor.exportar.posicao', { n: fila.filter((x) => x.estado === 'espera').indexOf(j) + 1 })
                            : j.resultado
                              ? `${tamanhoLegivel(j.resultado.bytes, i18n.language)} · ${relogio(j.resultado.duracao)}`
                              : ''}
                        </span>
                      </div>
                    </div>
                    <div className="ed-job__bar">
                      <span className="ed-meter">
                        <span
                          className={cx('ed-meter__fill', j.estado === 'concluida' && 'ed-meter__fill--ok')}
                          style={{ width: `${Math.round((j.estado === 'concluida' ? 1 : j.progresso?.fraccao ?? 0) * 100)}%` }}
                        />
                      </span>
                      <span className="dx-num ed-exp__sub">{Math.round((j.estado === 'concluida' ? 1 : j.progresso?.fraccao ?? 0) * 100)}%</span>
                    </div>
                    <div className="ed-job__row">
                      <div className="ed-steps">
                        {passos(j).map((s) => (
                          <span key={s.id} className={cx('ed-step dx-num', `ed-step--${s.estado}`)}>
                            {t(`editor.exportar.passos.${s.id}`, { l: j.legendas ?? '' })}
                          </span>
                        ))}
                      </div>
                      <span className="dx-spacer" />
                      {j.estado === 'espera' && i > 0 && (
                        <button
                          type="button"
                          className="ed-btn ed-btn--sm"
                          onClick={() =>
                            setFila((f) => {
                              const n = f.filter((x) => x.id !== j.id)
                              const primeiro = n.findIndex((x) => x.estado === 'espera')
                              n.splice(primeiro < 0 ? n.length : primeiro, 0, j)
                              return n
                            })
                          }
                        >
                          {t('editor.exportar.prioridade')}
                        </button>
                      )}
                      {j.estado === 'a-exportar' && (
                        <button type="button" className="ed-btn ed-btn--sm ed-btn--danger" onClick={() => controlo.current?.abort()}>
                          {t('editor.exportar.cancelar')}
                        </button>
                      )}
                      {j.estado === 'espera' && (
                        <button type="button" className="ed-btn ed-btn--sm ed-btn--danger" onClick={() => mudar(j.id, { estado: 'cancelada' })}>
                          {t('editor.exportar.cancelar')}
                        </button>
                      )}
                      {j.resultado && (
                        <a className="ed-btn ed-btn--sm" href={j.resultado.url} download={j.resultado.nome}>
                          {t('editor.exportar.descarregarDeNovo')}
                        </a>
                      )}
                    </div>
                    {j.erro && <p className="ed-exp__sub ed-tone--erro">{j.erro}</p>}
                    {j.resultado &&
                      (j.resultado.audio ? (
                        <audio className="ed-job__result" src={j.resultado.url} controls data-studio="render" />
                      ) : (
                        <video className="ed-job__result" src={j.resultado.url} controls playsInline data-studio="render" />
                      ))}
                  </div>
                ))}
              </div>
            </>
          )}

          {aba !== 'predefinicoes' && (
            <>
              <div className="ed-exp__head">
                <h2>{t('editor.exportar.historico')}</h2>
                <span className="ed-exp__sub">{t('editor.exportar.historicoNota')}</span>
                <span className="dx-spacer" />
                {historico.length > 0 && (
                  <button type="button" className="ed-exp__link" onClick={() => void bd.limparExportacoes().then(recarregarHistorico)}>
                    {t('editor.exportar.limparHistorico')}
                  </button>
                )}
              </div>
              {tabela}
            </>
          )}

          {aba === 'predefinicoes' && (
            <div className="ed-exp__table dx-table-wrap">
              <table className="dx-table">
                <thead>
                  <tr>
                    <th>{t('editor.exportar.colunas.predefinicao')}</th>
                    <th>{t('editor.exportar.colunas.formato')}</th>
                    <th>{t('editor.exportar.colunas.saida')}</th>
                  </tr>
                </thead>
                <tbody>
                  {PREDEFINICOES.map((x) => (
                    <tr key={x.id}>
                      <td>{t(`editor.exportar.presets.${x.id}.nome`)}</td>
                      <td className="dx-num">{x.soAudio ? t('editor.exportar.formatoAudio') : t('editor.exportar.formatoVideo')}</td>
                      <td className="dx-num">{t(`editor.exportar.presets.${x.id}.spec`)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="ed-exp__sub ed-exp__note">{t('editor.exportar.predefinicoesNota')}</p>
            </div>
          )}
        </div>

        <aside className="ed-exp__side">
          <div className="dx-card ed-exp__form" ref={formulario}>
            <h2 className="ed-exp__h">{t('editor.exportar.nova')}</h2>
            {!suportado && <Alert tone="warning">{t('studio.edicao.semWebCodecs')}</Alert>}
            <div className="dx-field">
              <label className="dx-field__label" htmlFor="ed-exp-origem">
                {t('editor.exportar.origem')}
              </label>
              <Select id="ed-exp-origem" value={origemId} onChange={(e) => setOrigemId(e.target.value)}>
                <option value={projecto.id}>{t('editor.exportar.projectoAberto', { titulo: projecto.titulo, id: projecto.id.slice(-6) })}</option>
                {lista
                  .filter((r) => r.id !== projecto.id)
                  .map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.id.slice(-6)} · {r.projecto.titulo}
                    </option>
                  ))}
              </Select>
            </div>

            <fieldset className="ed-presets">
              <legend className="dx-field__label">{t('editor.exportar.predefinicao')}</legend>
              {PREDEFINICOES.map((x) => (
                <label key={x.id} className={cx('ed-preset', preset === x.id && 'ed-preset--on')}>
                  <input type="radio" name="ed-preset" value={x.id} checked={preset === x.id} onChange={() => setPreset(x.id)} />
                  <span className="ed-preset__name">{t(`editor.exportar.presets.${x.id}.nome`)}</span>
                  <span className="dx-num ed-exp__sub">{t(`editor.exportar.presets.${x.id}.spec`)}</span>
                </label>
              ))}
            </fieldset>

            <div className="dx-field">
              <span className="dx-field__label">{t('editor.exportar.incluir')}</span>
              <div className={cx('ed-include', !!legendas && 'ed-include--on')}>
                <Checkbox
                  label={t('editor.exportar.legendas', { l: legendas || linguas[0] || '' })}
                  checked={!!legendas}
                  disabled={pr.soAudio || !linguas.length}
                  onChange={(e) => setLegendas(e.target.checked ? linguas[0] ?? '' : '')}
                />
                {linguas.length > 1 && legendas && (
                  <Select aria-label={t('editor.exportar.linguaLegendas')} value={legendas} onChange={(e) => setLegendas(e.target.value)}>
                    {linguas.map((l) => (
                      <option key={l} value={l}>
                        {l}
                      </option>
                    ))}
                  </Select>
                )}
              </div>
              <div className={cx('ed-include', comMarca && 'ed-include--on')}>
                <Checkbox label={t('editor.exportar.marcaDeAgua')} checked={comMarca} disabled={pr.soAudio} onChange={(e) => setComMarca(e.target.checked)} />
              </div>
              {!linguas.length && <span className="dx-field__hint">{t('editor.exportar.semTranscricao')}</span>}
            </div>

            <div className="dx-field">
              <label className="dx-field__label" htmlFor="ed-exp-destino">
                {t('editor.exportar.destino')}
              </label>
              <Select id="ed-exp-destino" value={destino} onChange={(e) => setDestino(e.target.value as Destino)} data-studio="destino">
                <option value="descarregar">{t('editor.exportar.descarregar')}</option>
                <option value="biblioteca">{t('editor.exportar.biblioteca')}</option>
              </Select>
            </div>

            <div className="ed-estimate">
              <span className="dx-num">{t('editor.exportar.estimativa')}</span>
              <strong className="dx-num">
                {tamanhoLegivel(tamanhoEstimado(pr, dur), i18n.language)} · {t('editor.exportar.tempo', { n: Math.max(1, Math.ceil(tempoEstimado(pr, dur, fps) / 60)) })}
              </strong>
            </div>
            {!fps && !pr.soAudio && <span className="dx-field__hint">{t('editor.exportar.estimativaNota')}</span>}

            <Button
              variant="primary"
              size="lg"
              block
              disabled={!suportado || dur <= 0 || semSom || aGuardar}
              busy={aGuardar}
              data-studio={destino === 'biblioteca' ? 'guardar-biblioteca' : 'exportar'}
              onClick={() => void colocar()}
            >
              {t('editor.exportar.colocarNaFila')}
            </Button>
            {semSom && <span className="dx-field__hint">{t('studio.erros.semAudio')}</span>}
            {guardado && (
              <p className="ed-exp__ok" role="status" data-studio="guardado">
                {guardado}
              </p>
            )}
          </div>
        </aside>
      </div>
    </>
  )
}
