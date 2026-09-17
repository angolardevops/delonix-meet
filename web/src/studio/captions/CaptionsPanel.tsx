/**
 * «Legendas e tradução», na grelha do template: idiomas à esquerda; vídeo e
 * transcrição editável ao centro; assistente, motor, estilo e residência à
 * direita.
 *
 * Transcrição local com tempos por palavra, transcrição que CORTA o vídeo,
 * palavras de preenchimento, trechos fora do microfone, tradução pelo servidor
 * (≤ 500 caracteres por pedido), estilos e SRT/VTT.
 *
 * O que NÃO está, porque não existe por trás: publicar legendas no servidor,
 * dobragem/TTS e a sua latência, o nó de transcodificação, a confiança média
 * do servidor e as línguas que o servidor não traduz (Umbundu, Kimbundu,
 * Kikongo, Mandarim).
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ROTAS_POR_PORTAR } from '../../capabilities'
import { apiErrorMessage, isAbort, recordingTranscript, translateCaption } from '../../api'
import { useShell } from '../../components/shellContext'
import { Alert, cx, IconButton, Select, TextArea } from '../../ui/kit'
import { misturar } from '../edit/mistura'
import Preview from '../edit/Preview'
import type { TranscriptSegment } from '../../api'
import type { Cue, Edicao, Legendas, ModoDeLegenda, Projecto } from '../edit/projecto'
import { rmsPorJanela, trechosForaDoMicrofone } from '../edit/sinal'
import type { TrechoFraco } from '../edit/sinal'
import type { Leitor } from '../edit/useLeitor'
import { nomeDeFicheiro, tamanhoLegivel } from '../exports/predefinicoes'
import {
  contarPreenchimento,
  editarTextoDaCue,
  encontrarPreenchimento,
  intervalosDasPalavras,
  LINGUAS_DE_TRADUCAO,
  palavrasDasCues,
  palavrasParaCues,
  paraSrt,
  paraVtt,
  REGRAS,
  relogio,
  traduzirCues,
} from './legendas'
import { cuesDoServidor, fonteDaBiblioteca } from './servidor'
import { apagarCacheDoModelo, espacoDoModelo, modeloDisponivel, paraDezasseisK, transcrever } from './transcricao'

type EstadoDaTranscricao =
  | { fase: 'parado' }
  | { fase: 'a-misturar' }
  | { fase: 'modelo'; pct: number }
  | { fase: 'a-transcrever'; fraccao: number }
  | { fase: 'erro'; msg: string }

type EstadoDaTraducao = { feitas: number; total: number } | { erro: string }

const JANELA = 0.05

export function descarregarTexto(texto: string, nome: string, tipo: string) {
  const url = URL.createObjectURL(new Blob([texto], { type: tipo }))
  const a = document.createElement('a')
  a.href = url
  a.download = nome
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

export default function CaptionsPanel({
  projecto: p,
  leitor,
  aplicar,
  lerBlob,
  onErro,
  marcaDeAgua,
  termosExtra = [],
}: {
  /** Palavras de preenchimento que o LLM local encontrou nesta transcrição. */
  termosExtra?: readonly string[]
  projecto: Projecto
  leitor: Leitor
  aplicar: (e: Edicao, chave?: string | null) => void
  lerBlob: (id: string) => Promise<Blob>
  onErro: (m: string) => void
  marcaDeAgua: string
}) {
  const { t, i18n } = useTranslation()
  const { user } = useShell()
  const pRef = useRef(p)
  pRef.current = p
  const leg = p.legendas
  const [linguaOrigem, setLinguaOrigem] = useState<string>(leg?.lingua ?? (i18n.language.split('-')[0] || 'pt'))
  const [transcricao, setTranscricao] = useState<EstadoDaTranscricao>({ fase: 'parado' })
  const [modelo, setModelo] = useState<boolean | null>(null)
  const [espaco, setEspaco] = useState<{ modelo: number; usado: number | null; quota: number | null } | null>(null)
  const [traducoes, setTraducoes] = useState<Record<string, EstadoDaTraducao>>({})
  const [linguaVista, setLinguaVista] = useState<string | null>(null)
  const [seleccao, setSeleccao] = useState<Set<number>>(new Set())
  const [buracos, setBuracos] = useState<Set<number>>(new Set())
  const [mostrarSilencios, setMostrarSilencios] = useState(false)
  const [aEditar, setAEditar] = useState<{ i: number; texto: string } | null>(null)
  const [trechos, setTrechos] = useState<TrechoFraco[] | null>(null)
  const [aAnalisar, setAAnalisar] = useState(false)
  const [aAcrescentar, setAAcrescentar] = useState(false)
  const pedidos = useRef(new Map<string, AbortController>())
  // Transcrição que o servidor já tem da gravação da biblioteca (se a houver).
  const daBiblioteca = fonteDaBiblioteca(p)
  const [doServidor, setDoServidor] = useState<{ gravacao: string; segmentos: TranscriptSegment[]; lingua: string | null } | null>(null)
  useEffect(() => {
    if (leg || !daBiblioteca || !ROTAS_POR_PORTAR.recordingTranscript) return
    const ctl = new AbortController()
    recordingTranscript(daBiblioteca.gravacao, ctl.signal)
      .then((r) => {
        if (r.status === 'ready' && r.segments.length) setDoServidor({ gravacao: daBiblioteca.gravacao, segmentos: r.segments, lingua: r.language })
      })
      // Sem transcrição no servidor (404, sem acesso, offline) fica o caminho do browser.
      .catch(() => undefined)
    return () => ctl.abort()
  }, [leg, daBiblioteca?.gravacao]) // eslint-disable-line react-hooks/exhaustive-deps

  function usarTranscricaoDoServidor() {
    const origem = fonteDaBiblioteca(pRef.current)
    if (!doServidor || !origem || origem.gravacao !== doServidor.gravacao) return
    const lingua = doServidor.lingua?.split('-')[0] || linguaOrigem
    // Sem orador: quem fala não vem nos segmentos (o texto pode já o trazer), e
    // pôr o nome de quem abriu o projecto seria atribuir-lhe a fala de outros.
    const cues = cuesDoServidor(pRef.current, origem.fonteId, doServidor.segmentos)
    if (!cues.length) {
      onErro(t('editor.legendas.servidorVazio'))
      return
    }
    setLinguaOrigem(lingua)
    aplicar({ tipo: 'legendas', legendas: { lingua, cues, estimadas: true, traducoes: {} } })
    setSeleccao(new Set())
    setBuracos(new Set())
    setLinguaVista(null)
  }

  useEffect(() => {
    const ctl = new AbortController()
    void modeloDisponivel(ctl.signal).then(setModelo)
    void espacoDoModelo().then(setEspaco)
    const mapa = pedidos.current
    return () => {
      ctl.abort()
      for (const c of mapa.values()) c.abort()
    }
  }, [])

  const vista = linguaVista && leg && linguaVista !== leg.lingua ? linguaVista : leg?.lingua ?? null
  const eOrigem = !!leg && vista === leg.lingua
  const cues: Cue[] = !leg ? [] : eOrigem ? leg.cues : leg.traducoes[vista ?? ''] ?? []
  const palavras = useMemo(() => (leg ? palavrasDasCues(leg.cues) : []), [leg])
  const enchimentos = useMemo(() => (leg ? encontrarPreenchimento(palavras.map((x) => x.palavra), leg.lingua, termosExtra) : []), [palavras, leg, termosExtra])
  const indicesEnchimento = useMemo(() => new Set(enchimentos.flatMap((o) => o.indices)), [enchimentos])
  const gaps = useMemo(() => {
    const out: { i: number; inicio: number; fim: number }[] = []
    for (let i = 1; i < palavras.length; i++) {
      const a = palavras[i - 1].palavra.fim
      const b = palavras[i].palavra.inicio
      if (b - a >= 1) out.push({ i, inicio: a + 0.1, fim: b - 0.1 })
    }
    return out
  }, [palavras])

  const aTranscrever = transcricao.fase === 'a-misturar' || transcricao.fase === 'modelo' || transcricao.fase === 'a-transcrever'

  async function transcreverAgora() {
    const ctl = new AbortController()
    pedidos.current.get('transcricao')?.abort()
    pedidos.current.set('transcricao', ctl)
    setTranscricao({ fase: 'a-misturar' })
    try {
      const m = await misturar(pRef.current, lerBlob, {}, ctl.signal)
      if (!m) throw new Error(t('studio.erros.semAudio'))
      const pcm = await paraDezasseisK(m.canais, m.taxa)
      setTranscricao({ fase: 'modelo', pct: 0 })
      const r = await transcrever(
        pcm,
        linguaOrigem,
        0,
        {
          aoModelo: (pct) => setTranscricao({ fase: 'modelo', pct }),
          aoProgredir: (fraccao) => setTranscricao({ fase: 'a-transcrever', fraccao }),
        },
        ctl.signal,
      )
      const novas = palavrasParaCues(r.palavras, REGRAS, user?.username)
      const l: Legendas = { lingua: linguaOrigem, cues: novas, estimadas: r.estimadas, traducoes: {} }
      aplicar({ tipo: 'legendas', legendas: l })
      setSeleccao(new Set())
      setBuracos(new Set())
      setLinguaVista(null)
      setTranscricao({ fase: 'parado' })
      void espacoDoModelo().then(setEspaco)
    } catch (e) {
      if (isAbort(e)) setTranscricao({ fase: 'parado' })
      else setTranscricao({ fase: 'erro', msg: apiErrorMessage(e, t('editor.legendas.erroTranscricao')) })
    } finally {
      pedidos.current.delete('transcricao')
    }
  }

  async function traduzir(lingua: string) {
    const origem = pRef.current.legendas
    if (!origem?.cues.length) return
    const ctl = new AbortController()
    pedidos.current.get(lingua)?.abort()
    pedidos.current.set(lingua, ctl)
    setTraducoes((m) => ({ ...m, [lingua]: { feitas: 0, total: origem.cues.length } }))
    try {
      const out = await traduzirCues(
        origem.cues,
        lingua,
        (texto, alvo) => translateCaption(texto, alvo).then((r) => r.text),
        (feitas, total) => setTraducoes((m) => ({ ...m, [lingua]: { feitas, total } })),
        ctl.signal,
      )
      const actual = pRef.current.legendas
      // Se a transcrição mudou entretanto (novo corte, nova transcrição), a
      // tradução já não corresponde às cues — não se grava desalinhada.
      if (!actual || actual.cues.length !== origem.cues.length || actual.cues.some((c, i) => c.id !== origem.cues[i].id)) {
        setTraducoes((m) => ({ ...m, [lingua]: { erro: t('editor.legendas.mudouEntretanto') } }))
        return
      }
      aplicar({ tipo: 'legendas', legendas: { ...actual, traducoes: { ...actual.traducoes, [lingua]: out } } })
      setTraducoes((m) => {
        const n = { ...m }
        delete n[lingua]
        return n
      })
    } catch (e) {
      if (!isAbort(e)) setTraducoes((m) => ({ ...m, [lingua]: { erro: apiErrorMessage(e, t('editor.legendas.erroTraducao')) } }))
      else
        setTraducoes((m) => {
          const n = { ...m }
          delete n[lingua]
          return n
        })
    } finally {
      pedidos.current.delete(lingua)
    }
  }

  function cortarSeleccao() {
    if (!leg) return
    const intervalos = intervalosDasPalavras(
      palavras.map((x) => x.palavra),
      seleccao,
    )
    for (const g of gaps) if (buracos.has(g.i)) intervalos.push({ inicio: g.inicio, fim: g.fim })
    if (!intervalos.length) return
    aplicar({ tipo: 'cortar-intervalos', intervalos })
    setSeleccao(new Set())
    setBuracos(new Set())
  }

  async function analisarMicrofone() {
    setAAnalisar(true)
    try {
      const m = await misturar(pRef.current, lerBlob)
      setTrechos(m ? trechosForaDoMicrofone(rmsPorJanela(m.canais, m.taxa, JANELA), JANELA) : [])
    } catch (e) {
      onErro(apiErrorMessage(e, t('studio.erros.analise')))
    } finally {
      setAAnalisar(false)
    }
  }

  function exportar(formato: 'srt' | 'vtt') {
    if (!vista || !cues.length) return
    const karaoke = p.estilo.modo === 'karaoke' && eOrigem
    const texto = formato === 'srt' ? paraSrt(cues) : paraVtt(cues, { karaoke, oradores: true })
    descarregarTexto(texto, nomeDeFicheiro(p.titulo, vista, formato), formato === 'srt' ? 'application/x-subrip' : 'text/vtt')
  }

  const seleccionadas = [...seleccao].sort((a, b) => a - b)
  const textoSeleccao = seleccionadas.map((i) => palavras[i]?.palavra.texto).join(' ')
  const linguasDisponiveis = LINGUAS_DE_TRADUCAO.filter((l) => l !== leg?.lingua && !leg?.traducoes[l] && !traducoes[l])
  const resumo = contarPreenchimento(enchimentos)
  const selMostrada = seleccao.size + buracos.size
  const linguasActivas = leg ? [...new Set([...Object.keys(leg.traducoes), ...Object.keys(traducoes)])] : []

  function estadoDaLingua(l: string): { texto: string; pct: number; tom: 'ok' | 'aviso' | 'erro' } {
    const e = traducoes[l]
    if (e && 'erro' in e) return { texto: t('editor.legendas.estado.erro'), pct: 0, tom: 'erro' }
    if (e) return { texto: t('editor.legendas.estado.aGerar'), pct: e.total ? e.feitas / e.total : 0, tom: 'aviso' }
    return { texto: t('editor.legendas.estado.pronto'), pct: 1, tom: 'ok' }
  }

  const pctOrigem = !leg
    ? transcricao.fase === 'a-transcrever'
      ? transcricao.fraccao
      : 0
    : 1

  return (
    <div className="ed-caps">
      <aside className="ed-col ed-col--left ed-caps__langs" aria-label={t('editor.legendas.idiomas')}>
        <div className="ed-row ed-row--between">
          <h2 className="ed-h">{t('editor.legendas.idiomas')}</h2>
          <span className="dx-num ed-src__sub">{t('editor.legendas.activos', { count: leg ? 1 + Object.keys(leg.traducoes).length : 0 })}</span>
        </div>

        <div className={cx('ed-lang', vista === leg?.lingua && 'ed-lang--on', !leg && 'ed-lang--on')} data-lingua="origem">
          <div className="ed-lang__row">
            <span className="ed-lang__code dx-num">{linguaOrigem}</span>
            <Select className="ed-lang__select" aria-label={t('editor.legendas.linguaOrigem')} value={linguaOrigem} disabled={aTranscrever || !!leg} onChange={(e) => setLinguaOrigem(e.target.value)}>
              {LINGUAS_DE_TRADUCAO.map((l) => (
                <option key={l} value={l}>
                  {t(`editor.linguas.${l}`)}
                </option>
              ))}
            </Select>
            <span className={cx('ed-lang__state dx-num', leg && 'ed-tone--ok')}>{leg ? t('editor.legendas.estado.origem') : aTranscrever ? t('editor.legendas.estado.aGerar') : t('editor.legendas.estado.porFazer')}</span>
          </div>
          <div className="ed-lang__bar">
            <span className="ed-meter">
              <span className={cx('ed-meter__fill', leg && 'ed-meter__fill--ok')} style={{ width: `${Math.round(pctOrigem * 100)}%` }} />
            </span>
            <span className="dx-num ed-src__sub">{Math.round(pctOrigem * 100)}%</span>
          </div>
          <button type="button" className="ed-lang__modes dx-num" onClick={() => leg && setLinguaVista(leg.lingua)} disabled={!leg}>
            {t('editor.legendas.modosOrigem')}
          </button>
        </div>

        {leg &&
          linguasActivas.map((l) => {
            const e = estadoDaLingua(l)
            const erro = traducoes[l] && 'erro' in traducoes[l] ? (traducoes[l] as { erro: string }).erro : null
            const aGerar = !!traducoes[l] && !erro
            return (
              <div key={l} className={cx('ed-lang', vista === l && 'ed-lang--on')} data-lingua={l}>
                <div className="ed-lang__row">
                  <span className="ed-lang__code dx-num">{l}</span>
                  <button type="button" className="ed-lang__name" disabled={!leg.traducoes[l]} onClick={() => setLinguaVista(l)}>
                    {t(`editor.linguas.${l}`)}
                  </button>
                  <span className={cx('ed-lang__state dx-num', `ed-tone--${e.tom}`)}>{aGerar ? `${Math.round(e.pct * 100)}%` : e.texto}</span>
                </div>
                <div className="ed-lang__bar">
                  <span className="ed-meter">
                    <span className={cx('ed-meter__fill', e.tom === 'ok' && 'ed-meter__fill--ok', e.tom === 'aviso' && 'ed-meter__fill--warn')} style={{ width: `${Math.round(e.pct * 100)}%` }} />
                  </span>
                  <span className="dx-num ed-src__sub">{Math.round(e.pct * 100)}%</span>
                </div>
                {erro && <span className="ed-src__sub ed-tone--erro">{erro}</span>}
                <div className="ed-row">
                  <button
                    type="button"
                    className={cx('ed-lang__modes dx-num', p.estilo.segundaLingua === l && 'ed-tone--ok')}
                    aria-pressed={p.estilo.segundaLingua === l}
                    disabled={!leg.traducoes[l]}
                    onClick={() => aplicar({ tipo: 'estilo', patch: { segundaLingua: p.estilo.segundaLingua === l ? null : l } })}
                  >
                    {t('editor.legendas.modosTraducao')}
                    {p.estilo.segundaLingua === l ? ` · ${t('editor.legendas.segunda')}` : ''}
                  </button>
                  <span className="dx-spacer" />
                  {aGerar ? (
                    <button type="button" className="ed-link" onClick={() => pedidos.current.get(l)?.abort()}>
                      {t('editor.legendas.cancelar')}
                    </button>
                  ) : (
                    <IconButton
                      icon="trash"
                      bare
                      label={t('editor.legendas.removerLingua', { l })}
                      onClick={() => {
                        setTraducoes((m) => {
                          const n = { ...m }
                          delete n[l]
                          return n
                        })
                        if (leg.traducoes[l]) {
                          const n = { ...leg.traducoes }
                          delete n[l]
                          aplicar({ tipo: 'legendas', legendas: { ...leg, traducoes: n } })
                        }
                      }}
                    />
                  )}
                </div>
              </div>
            )
          })}

        {aAcrescentar ? (
          <div className="ed-lang">
            <Select
              aria-label={t('editor.legendas.acrescentar')}
              value=""
              autoFocus
              onChange={(e) => {
                if (e.target.value) void traduzir(e.target.value)
                setAAcrescentar(false)
              }}
              onBlur={() => setAAcrescentar(false)}
              data-studio="traduzir"
            >
              <option value="">{t('editor.legendas.escolherLingua')}</option>
              {linguasDisponiveis.map((l) => (
                <option key={l} value={l}>
                  {t(`editor.linguas.${l}`)}
                </option>
              ))}
            </Select>
          </div>
        ) : (
          <button type="button" className="ed-add" disabled={!leg || !linguasDisponiveis.length} onClick={() => setAAcrescentar(true)} data-studio="acrescentar-lingua">
            {t('editor.legendas.acrescentar')}
          </button>
        )}
        <p className="ed-src__sub">{t('editor.legendas.linguasServidor')}</p>
      </aside>

      <section className="ed-caps__centre" aria-label={t('editor.legendas.transcricao')}>
        <Preview
          projecto={p}
          leitor={leitor}
          lingua={vista}
          marcaDeAgua={marcaDeAgua}
          forma="legendas"
          topo={<span className="ed-badge dx-num">{[user?.username, vista ?? linguaOrigem].filter(Boolean).join(' · ')}</span>}
        />

        <div className="ed-transcript__head">
          <h2 className="ed-h">{t('editor.legendas.transcricao')}</h2>
          <span className="dx-num ed-src__sub">{t('editor.legendas.cortaVideo')}</span>
          <span className="dx-spacer" />
          <button type="button" className={cx('ed-btn ed-btn--sm', mostrarSilencios && 'ed-btn--on')} aria-pressed={mostrarSilencios} disabled={!eOrigem} onClick={() => setMostrarSilencios(!mostrarSilencios)}>
            {t('editor.legendas.mostrarSilencios')}
          </button>
          {leg && (
            <Select className="ed-lang-pick" aria-label={t('editor.legendas.linguaVista')} value={vista ?? ''} onChange={(e) => setLinguaVista(e.target.value)}>
              <option value={leg.lingua}>{leg.lingua}</option>
              {Object.keys(leg.traducoes).map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </Select>
          )}
        </div>

        <div className="ed-transcript">
          {!leg ? (
            <div className="ed-transcript__empty">
              <p className="st-note">{modelo === false ? t('editor.legendas.semTranscricaoSemModelo') : t('editor.legendas.semTranscricao')}</p>
              {doServidor && (
                <>
                  <p className="st-note">{t('editor.legendas.servidorDisponivel', { count: doServidor.segmentos.length })}</p>
                  <button type="button" className="ed-btn ed-btn--primary" onClick={usarTranscricaoDoServidor} data-studio="usar-transcricao-servidor">
                    {t('editor.legendas.usarServidor')}
                  </button>
                </>
              )}
            </div>
          ) : (
            <>
              {leg.estimadas && eOrigem && <Alert tone="warning">{t('editor.legendas.estimadas')}</Alert>}
              <ol className="ed-lines" data-studio="transcricao">
                {cues.map((c, ci) => {
                  const primeira = eOrigem ? palavras.findIndex((x) => x.cue === ci) : -1
                  return (
                    <li key={c.id} className="ed-line">
                      <button type="button" className="ed-line__t dx-num" onClick={() => leitor.buscar(c.inicio)}>
                        {relogio(c.inicio)}
                      </button>
                      <div className="ed-line__body">
                        {c.orador && <span className="ed-line__who">{c.orador}: </span>}
                        {aEditar?.i === ci ? (
                          <span className="ed-line__edit">
                            <TextArea value={aEditar.texto} rows={2} aria-label={t('editor.legendas.editarLinha')} onChange={(e) => setAEditar({ i: ci, texto: e.target.value })} />
                            <span className="ed-row">
                              <button
                                type="button"
                                className="ed-btn ed-btn--primary ed-btn--sm"
                                onClick={() => {
                                  const nova = editarTextoDaCue(c, aEditar.texto)
                                  const lista = cues.map((x, k) => (k === ci ? nova : x))
                                  aplicar({
                                    tipo: 'legendas',
                                    legendas: eOrigem ? { ...leg, cues: lista } : { ...leg, traducoes: { ...leg.traducoes, [vista!]: lista } },
                                  })
                                  setAEditar(null)
                                }}
                              >
                                {t('editor.legendas.guardarLinha')}
                              </button>
                              <button type="button" className="ed-btn ed-btn--sm" onClick={() => setAEditar(null)}>
                                {t('editor.legendas.cancelar')}
                              </button>
                            </span>
                          </span>
                        ) : eOrigem ? (
                          palavras.map((x, wi) => {
                            if (x.cue !== ci) return null
                            const gap = mostrarSilencios && wi > primeira ? gaps.find((g) => g.i === wi) : undefined
                            return (
                              <span key={wi}>
                                {gap && (
                                  <button
                                    type="button"
                                    className={cx('ed-gap', buracos.has(gap.i) && 'ed-word--sel')}
                                    aria-pressed={buracos.has(gap.i)}
                                    onClick={() =>
                                      setBuracos((s) => {
                                        const n = new Set(s)
                                        if (n.has(gap.i)) n.delete(gap.i)
                                        else n.add(gap.i)
                                        return n
                                      })
                                    }
                                  >
                                    {t('editor.legendas.pausa', { s: (gap.fim - gap.inicio + 0.2).toFixed(1) })}
                                  </button>
                                )}
                                <button
                                  type="button"
                                  className={cx(
                                    'ed-word',
                                    seleccao.has(wi) && 'ed-word--sel',
                                    indicesEnchimento.has(wi) && 'ed-word--filler',
                                    leitor.tempo >= x.palavra.inicio && leitor.tempo < x.palavra.fim && 'ed-word--now',
                                  )}
                                  aria-pressed={seleccao.has(wi)}
                                  onClick={(ev) => {
                                    setSeleccao((s) => {
                                      const n = new Set(s)
                                      if (ev.shiftKey && s.size) {
                                        const ult = Math.max(...s)
                                        const [a, b] = ult < wi ? [ult, wi] : [wi, ult]
                                        for (let k = a; k <= b; k++) n.add(k)
                                      } else if (n.has(wi)) n.delete(wi)
                                      else n.add(wi)
                                      return n
                                    })
                                  }}
                                  onDoubleClick={() => leitor.buscar(x.palavra.inicio)}
                                >
                                  {x.palavra.texto}
                                </button>{' '}
                              </span>
                            )
                          })
                        ) : (
                          <span>{c.texto}</span>
                        )}
                      </div>
                      {aEditar?.i !== ci && <IconButton icon="edit" bare className="ed-line__pen" label={t('editor.legendas.editarLinha')} onClick={() => setAEditar({ i: ci, texto: c.texto })} />}
                    </li>
                  )
                })}
              </ol>
            </>
          )}
          {eOrigem && (
            <div className="ed-selbar" data-studio="seleccao">
              <span className="dx-num ed-src__sub">{t('editor.legendas.seleccao')}</span>
              <span className="ed-selbar__txt">
                {selMostrada ? t('editor.legendas.seleccionadas', { texto: textoSeleccao.slice(0, 60), count: selMostrada }) : resumo.length ? t('editor.legendas.enchimentos', { count: enchimentos.length }) : t('editor.legendas.dica')}
              </span>
              <span className="dx-spacer" />
              {resumo.length > 0 && !selMostrada && (
                <button type="button" className="ed-btn ed-btn--sm" onClick={() => setSeleccao(new Set(indicesEnchimento))}>
                  {t('editor.legendas.seleccionarEnchimento')}
                </button>
              )}
              {selMostrada > 0 && (
                <button type="button" className="ed-btn ed-btn--sm" onClick={() => (setSeleccao(new Set()), setBuracos(new Set()))}>
                  {t('editor.legendas.limpar')}
                </button>
              )}
              <button type="button" className="ed-btn ed-btn--primary ed-btn--sm" disabled={!selMostrada} onClick={cortarSeleccao} data-studio="apagar-corte">
                {t('editor.legendas.apagarECortar')}
              </button>
            </div>
          )}
        </div>
      </section>

      <aside className="ed-col ed-col--right ed-caps__tools" aria-label={t('editor.legendas.ferramentas')}>
        <section className="ed-card ed-card--accent">
          <div className="ed-card__title ed-row">
            <span>{t('editor.assistente.titulo')}</span>
            <span className="ed-pill dx-num">{t('editor.assistente.local')}</span>
          </div>
          {trechos === null ? (
            <>
              <p className="ed-text">{t('editor.assistente.explica')}</p>
              <div className="ed-grid2">
                <button type="button" className="ed-btn ed-btn--primary" disabled={aAnalisar || !p.clips.length} onClick={() => void analisarMicrofone()} data-studio="analisar-microfone">
                  {aAnalisar ? t('editor.assistente.aAnalisar') : t('editor.assistente.analisar')}
                </button>
              </div>
            </>
          ) : trechos.length === 0 ? (
            <>
              <p className="ed-text">{t('editor.assistente.nada')}</p>
              <div className="ed-grid2">
                <button type="button" className="ed-btn" onClick={() => setTrechos(null)}>
                  {t('editor.assistente.ignorar')}
                </button>
              </div>
            </>
          ) : (
            <>
              <p className="ed-text">
                {t('editor.assistente.encontrou', { count: trechos.length, de: relogio(trechos[0].inicio), ate: relogio(trechos[trechos.length - 1].fim) })}
              </p>
              <div className="ed-grid2">
                <button
                  type="button"
                  className="ed-btn ed-btn--primary"
                  onClick={() => {
                    for (const tr of trechos) aplicar({ tipo: 'ganho-intervalo', faixa: 'A1', inicio: tr.inicio, fim: tr.fim, ganhoDb: tr.ganhoDb })
                    setTrechos(null)
                  }}
                >
                  {t('editor.assistente.aplicar')}
                </button>
                <button type="button" className="ed-btn" onClick={() => setTrechos(null)}>
                  {t('editor.assistente.ignorar')}
                </button>
              </div>
            </>
          )}
        </section>

        <section className="ed-card">
          <div className="ed-card__title">{t('editor.motor.titulo')}</div>
          <div className="ed-engine ed-engine--on">
            <span className="ed-radio" aria-hidden="true" />
            <span className="ed-engine__txt">
              <span className="ed-engine__name">{t('editor.motor.local')}</span>
              <span className="dx-num ed-src__sub">{t('editor.motor.cache', { tamanho: tamanhoLegivel(espaco?.modelo ?? 0, i18n.language) })}</span>
            </span>
          </div>
          {modelo === false && <p className="ed-src__sub ed-tone--erro">{t('editor.motor.semModelo')}</p>}
          {transcricao.fase === 'erro' && <p className="ed-src__sub ed-tone--erro">{transcricao.msg}</p>}
          {aTranscrever && (
            <span className="dx-num ed-src__sub" role="status">
              {transcricao.fase === 'a-misturar'
                ? t('editor.motor.aMisturar')
                : transcricao.fase === 'modelo'
                  ? t('editor.motor.aCarregar', { pct: Math.round(transcricao.pct) })
                  : t('editor.motor.aTranscrever', { pct: Math.round((transcricao.fase === 'a-transcrever' ? transcricao.fraccao : 0) * 100) })}
            </span>
          )}
          <div className="ed-grid2">
            {aTranscrever ? (
              <button type="button" className="ed-btn" onClick={() => pedidos.current.get('transcricao')?.abort()}>
                {t('editor.legendas.cancelar')}
              </button>
            ) : (
              <button type="button" className="ed-btn ed-btn--primary" disabled={modelo === false || !p.clips.length} onClick={() => void transcreverAgora()} data-studio="transcrever">
                {leg ? t('editor.motor.retranscrever') : t('editor.motor.transcrever')}
              </button>
            )}
            <button type="button" className="ed-btn" disabled={!espaco?.modelo} onClick={() => void apagarCacheDoModelo().then(() => espacoDoModelo().then(setEspaco))}>
              {t('editor.motor.limparCache')}
            </button>
          </div>
          {leg && Object.keys(leg.traducoes).length > 0 && <p className="ed-src__sub">{t('editor.motor.perdeTraducoes')}</p>}
        </section>

        <section className="ed-card">
          <div className="ed-card__title">{t('editor.estilo.titulo')}</div>
          <div className="ed-grid2" role="group" aria-label={t('editor.estilo.titulo')}>
            {(['caixa', 'contorno', 'faixa', 'karaoke'] as ModoDeLegenda[]).map((m) => (
              <button key={m} type="button" className={cx('ed-btn', p.estilo.modo === m && 'ed-btn--sel')} aria-pressed={p.estilo.modo === m} onClick={() => aplicar({ tipo: 'estilo', patch: { modo: m } })}>
                {t(`editor.estilo.${m}`)}
              </button>
            ))}
          </div>
          <div className="ed-slider">
            <label className="ed-slider__label" htmlFor="ed-cap-tam">
              {t('editor.estilo.tamanho')}
            </label>
            <input
              id="ed-cap-tam"
              type="range"
              className="ed-range"
              min={16}
              max={72}
              step={1}
              value={p.estilo.tamanho}
              style={{ ['--pct' as string]: `${((p.estilo.tamanho - 16) / 56) * 100}%` }}
              onChange={(e) => aplicar({ tipo: 'estilo', patch: { tamanho: Number(e.target.value) } }, 'estilo:tamanho')}
            />
            <span className="dx-num ed-slider__val">{p.estilo.tamanho} px</span>
          </div>
          <div className="ed-grid2">
            <button type="button" className="ed-btn" disabled={!cues.length} onClick={() => exportar('srt')} data-studio="srt">
              {t('editor.estilo.srt')}
            </button>
            <button type="button" className="ed-btn" disabled={!cues.length} onClick={() => exportar('vtt')} data-studio="vtt">
              {t('editor.estilo.vtt')}
            </button>
          </div>
        </section>

        <section className="ed-card ed-card--end">
          <div className="ed-card__title">{t('editor.residencia.titulo')}</div>
          <p className="dx-num ed-mono">{t('editor.residencia.texto')}</p>
        </section>
      </aside>
    </div>
  )
}
