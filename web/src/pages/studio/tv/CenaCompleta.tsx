/**
 * DelonixStudioLive — o estúdio de TV numa janela: fontes, programa e pré,
 * transição, som, luz, alinhamento, destinos, carga do PC e Terminar emissão.
 *
 * É uma composição dos outros ecrãs sobre a MESMA sessão: cortar aqui é
 * cortar na mesa, o fader daqui é o da mesa de som.
 *
 * A carga do PC vem do browser: o custo MEDIDO de compor cada frame e, onde
 * existe, o estado de pressão da CPU (`PressureObserver`). A GPU não tem
 * medida no browser — não aparece.
 */
import { FormEvent, PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { teclaDeSobreposicao } from '../../../studio/tv/atalhos'
import { SOBREPOSICOES_DA_MESA } from '../../../studio/tv/macros'
import { fontePrincipal, planoDe } from '../../../studio/tv/mesa'
import { alturaDoMedidor, formatarDb } from '../../../studio/tv/som'
import { cx } from '../../../ui/kit'
import { CartaoDoBarramento, LugarVazio, type PropsDoEcra, SelosDoAr, useNomeDaFonte } from './comum'
import { ALVO_LUFS } from './MesaDeSomEcra'
import { BarrasDeNivel, BotaoTv, Cabeca, Cartao, Desde, duracao, Espaco, Fader, Medidor, MonitorDePlano, MonitorDoPrograma, TopoTv, useTique } from './pecas'

interface ItemDoAlinhamento {
  titulo: string
  duracaoS: number
  nota: string
}

const CHAVE_ALINHAMENTO = 'dx_studio_tv_alinhamento'

function lerAlinhamento(): { itens: ItemDoAlinhamento[]; actual: number } {
  try {
    const o = JSON.parse(localStorage.getItem(CHAVE_ALINHAMENTO) ?? '{}') as { itens?: unknown; actual?: unknown }
    const itens = Array.isArray(o.itens)
      ? o.itens
          .filter((x): x is ItemDoAlinhamento => !!x && typeof (x as ItemDoAlinhamento).titulo === 'string')
          .map((x) => ({ titulo: x.titulo.slice(0, 80), duracaoS: Math.max(0, Number(x.duracaoS) || 0), nota: String(x.nota ?? '').slice(0, 120) }))
      : []
    return { itens, actual: Math.min(Math.max(-1, Number(o.actual) || -1), itens.length - 1) }
  } catch {
    return { itens: [], actual: -1 }
  }
}

type Pressao = 'nominal' | 'fair' | 'serious' | 'critical'

/** O estado de pressão da CPU, quando o browser o dá; `null` quando não. */
function usePressaoDaCpu(): Pressao | null {
  const [p, setP] = useState<Pressao | null>(null)
  useEffect(() => {
    const PO = (globalThis as unknown as { PressureObserver?: new (cb: (r: { state: Pressao }[]) => void) => { observe: (s: string) => Promise<void>; disconnect: () => void } }).PressureObserver
    if (!PO) return
    const o = new PO((registos) => {
      const ultimo = registos[registos.length - 1]
      if (ultimo) setP(ultimo.state)
    })
    o.observe('cpu').catch(() => setP(null))
    return () => o.disconnect()
  }, [])
  return p
}

export default function CenaCompleta({ s, c }: PropsDoEcra) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const nome = useNomeDaFonte(s)
  const bus = s.registo.barramento()
  const m = s.mesa
  const noAr = c.directo.fase === 'no-ar'
  const mod = teclaDeSobreposicao()
  const pressao = usePressaoDaCpu()
  const custo = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  const [confirmar, setConfirmar] = useState(false)
  const [aTerminar, setATerminar] = useState(false)

  useEffect(() => {
    if (!s.som) void s.ligarSom()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useTique((agora) => {
    if (agora - ultimo.current < 500 || !custo.current) return
    ultimo.current = agora
    const ms = c.compRef.current?.custoDoFrameMs ?? 0
    custo.current.textContent = t('tv.cena.composicao', { ms: ms.toLocaleString(lang, { maximumFractionDigits: 1, minimumFractionDigits: 1 }) })
  })

  const idPgm = fontePrincipal(m.emCurso ? m.emCurso.para : m.programa)
  const idPre = fontePrincipal(m.previa)
  const nCamaras = s.registo.lista().filter((f) => f.tipo === 'camara').length
  const nPessoas = s.registo.lista().filter((f) => f.tipo === 'participante').length

  return (
    <div className="dx-stage tv tv--palco" data-tv-ecra="cena">
      <TopoTv titulo={t('tv.cena.titulo')} onVoltar={() => c.onNavegar(null)}>
        <SelosDoAr c={c} />
        <Espaco />
        <span className="tv-meta" data-tv="carga">
          <span ref={custo} />
          {pressao && <span>{` · ${t('tv.cena.cpu', { estado: t(`tv.cena.pressao.${pressao}`) })}`}</span>}
        </span>
        <BotaoTv onClick={() => c.onNavegar(null)}>
          {t('tv.nav.cenas')}
        </BotaoTv>
        {confirmar ? (
          <>
            <BotaoTv onClick={() => setConfirmar(false)}>
              {t('tv.cena.cancelar')}
            </BotaoTv>
            <BotaoTv
              variante="rec"
              data-tv="confirmar-terminar"
              disabled={aTerminar}
              onClick={async () => {
                setATerminar(true)
                try {
                  await c.onTerminarEmissao()
                } finally {
                  setATerminar(false)
                  setConfirmar(false)
                }
              }}
            >
              {t('tv.cena.confirmarTerminar')}
            </BotaoTv>
          </>
        ) : (
          <BotaoTv variante="rec" data-tv="terminar" disabled={!noAr} title={noAr ? undefined : t('tv.cena.naoNoAr')} onClick={() => setConfirmar(true)}>
            {t('tv.cena.terminar')}
          </BotaoTv>
        )}
      </TopoTv>

      <div className="tv-corpo tv-cena">
        <aside className="tv-col tv-col--esq" aria-label={t('tv.cena.fontes')}>
          <p className="tv-eyebrow">{t('tv.cena.fontes')}</p>
          <ul className="tv-rolar" style={{ listStyle: 'none', margin: 0, padding: 0, display: 'flex', flexDirection: 'column', gap: 7 }}>
            {Array.from({ length: 6 }, (_, i) => {
              const f = bus[i]
              return (
                <li key={f?.id ?? `vazio-${i}`}>
                  {f ? (
                    <CartaoDoBarramento s={s} fonte={f} numero={i + 1} onPrevia={() => s.accoes.previa(planoDe(f.id))} onAr={() => s.accoes.ar(planoDe(f.id))} />
                  ) : (
                    <LugarVazio numero={i + 1} onAbrir={() => c.onNavegar('fontes')} />
                  )}
                </li>
              )
            })}
          </ul>
          <Cartao variante="nota" style={{ marginTop: 'auto', padding: 8, gap: 4 }}>
            <span className="tv-t3" style={{ fontSize: 9.5 }}>
              {t('tv.cena.resumoFontes', { camaras: nCamaras, pessoas: nPessoas })}
            </span>
            <span className="tv-nota" style={{ fontSize: 8.5 }}>
              {t('tv.cena.resumoFontesNota')}
            </span>
          </Cartao>
        </aside>

        <section className="tv-centro" aria-label={t('tv.cena.rotulo')}>
          <div className="tv-cena__topo">
            <div className="tv-monitor tv-monitor--pgm">
              <div className="tv-monitor__cabeca">
                <span className="tv-monitor__estado">{t('tv.cena.programa')}</span>
                <span className="tv-monitor__nome">{idPgm ? nome(idPgm) : t('tv.corte.palcoDoEstudio')}</span>
                <span className="tv-monitor__lado">{m.programa && <Desde desde={m.noArDesde} />}</span>
              </div>
              <div className="tv-monitor__corpo">
                <MonitorDoPrograma compRef={c.compRef} devolverA={c.canvasHostRef} />
                <span className="tv-monitor__sobre">{noAr && <span className="tv-monitor__rotulo">{t('tv.corte.marcaAoVivo')}</span>}</span>
                <span className="tv-monitor__rodape">
                  {s.som && (
                    <BarrasDeNivel
                      n={10}
                      ler={() => {
                        const [l, r] = s.som!.nivelMestre()
                        return alturaDoMedidor(Math.max(l.picoDb, r.picoDb))
                      }}
                    />
                  )}
                </span>
              </div>
            </div>

            <div className="tv-cena__lado">
              <div className="tv-monitor tv-monitor--pre">
                <div className="tv-monitor__cabeca">
                  <span className="tv-monitor__estado">{t('tv.tally.previa')}</span>
                  <span className="tv-monitor__nome">{idPre ? nome(idPre) : t('tv.corte.semPrevia')}</span>
                </div>
                <div className="tv-monitor__corpo">
                  <MonitorDePlano fontes={s.registo} plano={m.previa} />
                </div>
              </div>
              <Cartao como="section" variante="painel" style={{ flex: 1, minHeight: 0, padding: 10, gap: 7 }} aria-label={t('tv.corte.transicao')}>
                <h2 className="tv-t3">{t('tv.corte.transicao')}</h2>
                <div className="tv-cortar-auto">
                  <button type="button" className="tv-tbar__botao" disabled={!m.previa} onClick={s.accoes.cortar} data-tv="cena-cortar">
                    {t('tv.transicoes.cortar')}
                  </button>
                  <button type="button" className="tv-tbar__botao tv-tbar__botao--auto" disabled={!m.previa} onClick={() => s.accoes.auto(m.transicao === 'cortar' ? 'misturar' : m.transicao)}>
                    {t('tv.cena.auto', { s: (m.duracaoMs / 1000).toLocaleString(lang, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}
                  </button>
                </div>
                <TBarHorizontal s={s} />
                <div className="tv-tags" style={{ marginTop: 'auto' }}>
                  {SOBREPOSICOES_DA_MESA.map((q, i) => (
                    <button
                      key={q}
                      type="button"
                      className="tv-tag"
                      style={{ cursor: 'pointer', background: 'transparent', ...(s.sobreposicaoLigada(q) ? { borderColor: 'var(--accent)', color: 'var(--accent)' } : {}) }}
                      aria-pressed={s.sobreposicaoLigada(q)}
                      disabled={q === 'sondagem' && !c.haSondagem}
                      onClick={() => s.definirSobreposicao(q, !s.sobreposicaoLigada(q))}
                    >
                      {t('tv.cena.chip', { nome: t(`tv.sobreposicoes.${q}`), tecla: `${mod}${i + 1}` })}
                    </button>
                  ))}
                </div>
              </Cartao>
            </div>
          </div>

          <div className="tv-cena__baixo">
            <SomResumo s={s} onAbrir={() => c.onNavegar('mesa-de-som')} />
            <Cartao como="section" variante="painel" style={{ padding: 11, gap: 9 }} aria-labelledby="tv-cena-luz-h">
              <Cabeca>
                <h2 id="tv-cena-luz-h" className="tv-t2">
                  {t('tv.cena.luz')}
                </h2>
                <span className="tv-mono-85">{t('tv.luz.semAgente')}</span>
                <BotaoTv aDireita mini onClick={() => c.onNavegar('iluminacao')}>
                  {t('tv.nav.iluminacao')}
                </BotaoTv>
              </Cabeca>
              <div className="tv-rolar" style={{ display: 'flex', flexDirection: 'column', gap: 6, flex: 1 }}>
                {s.registo.lista().length === 0 && <p className="tv-nota">{t('tv.luz.semFontes')}</p>}
                {s.registo.lista().map((f) => {
                  const k = s.registo.correccao(f.id)
                  return (
                    <div key={f.id} className="tv-luz-linha">
                      <span />
                      <span style={{ fontWeight: 600, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{f.nome}</span>
                      <span className="tv-dir tv-mono-85">{t('tv.luz.resumo', { ev: formatarDb(k.exposicao, lang), k: Math.round(k.temperatura).toLocaleString(lang) })}</span>
                    </div>
                  )
                })}
              </div>
              <Cabeca className="tv-mono-85" style={{ marginTop: 'auto' }}>
                <span>{t('tv.cena.correccaoPorSoftware')}</span>
                {s.registo.lista().some((f) => s.registo.correccao(f.id).equilibrio.some((x) => x !== 1)) && (
                  <span className="tv-dir tv-ok">{t('tv.cena.brancosAlinhados')}</span>
                )}
              </Cabeca>
            </Cartao>
          </div>
        </section>

        <aside className="tv-col tv-col--dir" style={{ gap: 10 }} aria-label={t('tv.cena.alinhamento')}>
          <Alinhamento />
          <Cartao variante="live" style={{ padding: 9, gap: 4 }} data-tv="destinos">
            <h2 className="tv-t3" style={{ fontSize: 10, color: 'var(--live-text)' }}>
              {t('tv.cena.destinos')}
            </h2>
            {c.destinos.length === 0 ? (
              <p className="tv-nota">{t('tv.cena.semDestinos')}</p>
            ) : (
              <ul className="tv-destinos">
                {c.destinos.map((d, i) => (
                  <li key={i}>
                    <span>{d.rotulo || d.url || t('tv.cena.destinoN', { n: i + 1 })}</span>
                    <span>{!d.chave.trim() ? t('tv.cena.semChave') : noAr ? t('tv.cena.noAr') : c.directo.fase === 'a-ligar' ? t('tv.cena.aLigar') : t('tv.cena.pronto')}</span>
                  </li>
                ))}
              </ul>
            )}
          </Cartao>
          <Cartao variante="nota" style={{ marginTop: 'auto', padding: 9, gap: 5 }}>
            <h3 className="tv-t3" style={{ fontSize: 10 }}>
              {t('tv.cena.tudoNumPc')}
            </h3>
            <p className="tv-nota" style={{ fontSize: 9, lineHeight: 1.5 }}>
              {t('tv.cena.tudoNumPcNota')}
            </p>
          </Cartao>
        </aside>
      </div>
    </div>
  )
}

function TBarHorizontal({ s }: { s: PropsDoEcra['s'] }) {
  const { t } = useTranslation()
  const pista = useRef<HTMLDivElement>(null)
  const barra = useRef<HTMLSpanElement>(null)
  const texto = useRef<HTMLSpanElement>(null)
  useTique(() => {
    const e = s.mesaRef.current
    const prog = e.emCurso ? (e.emCurso.inicio < 0 ? (e.tbarEmBaixo ? 1 - e.tbar : e.tbar) : Math.min(1, (Date.now() - e.emCurso.inicio) / e.emCurso.duracaoMs)) : 0
    if (barra.current) barra.current.style.width = `${Math.round(prog * 100)}%`
    if (texto.current) texto.current.textContent = t('tv.cena.tbar', { pct: Math.round(prog * 100) })
  })
  const arrastar = (e: ReactPointerEvent<HTMLDivElement>) => {
    const el = pista.current
    if (!el || !s.mesa.previa) return
    el.setPointerCapture(e.pointerId)
    const mover = (ev: PointerEvent) => {
      const r = el.getBoundingClientRect()
      const p = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width))
      s.accoes.tbar(s.mesaRef.current.tbarEmBaixo ? 1 - p : p)
    }
    mover(e.nativeEvent)
    const largar = () => {
      el.removeEventListener('pointermove', mover)
      el.removeEventListener('pointerup', largar)
    }
    el.addEventListener('pointermove', mover)
    el.addEventListener('pointerup', largar)
  }
  return (
    <div className="tv-hbar-t">
      <div ref={pista} className="tv-hbar-pista" role="presentation" onPointerDown={arrastar} data-tv="tbar-horizontal">
        <span ref={barra} style={{ width: 0 }} />
      </div>
      <span ref={texto} className="tv-mono-9" />
    </div>
  )
}

function SomResumo({ s, onAbrir }: { s: PropsDoEcra['s']; onAbrir: () => void }) {
  const { t, i18n } = useTranslation()
  const som = s.som
  const lufs = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (!som || agora - ultimo.current < 300 || !lufs.current) return
    ultimo.current = agora
    const l = som.sonoridadeAgora()
    const v = Number.isFinite(l.curta) ? l.curta : l.momentanea
    lufs.current.textContent = Number.isFinite(v) ? t('tv.cena.lufs', { v: formatarDb(v, i18n.language) }) : t('tv.cena.lufsSemSinal')
    lufs.current.className = cx('tv-mono-85', Number.isFinite(v) && Math.abs(v - ALVO_LUFS) <= 1 ? 'tv-ok' : 'tv-aviso')
  })
  const canais = som?.lista().slice(0, 5) ?? []
  return (
    <Cartao como="section" variante="painel" style={{ padding: 11, gap: 9 }} aria-labelledby="tv-cena-som-h" data-tv="cena-som">
      <Cabeca>
        <h2 id="tv-cena-som-h" className="tv-t2">
          {t('tv.cena.som')}
        </h2>
        <span ref={lufs} className="tv-mono-85" />
        <BotaoTv aDireita mini onClick={onAbrir}>
          {t('tv.som.contagem', { count: som?.lista().length ?? 0 })}
        </BotaoTv>
      </Cabeca>
      {!som ? (
        <div className="tv-vazio">{s.erroSom || t('tv.som.aLigar')}</div>
      ) : (
        <div className="tv-faders">
          {canais.map((x) => (
            <div key={x.id} className="tv-faders__f">
              <div className="tv-tira__fader">
                <Medidor ler={() => alturaDoMedidor(som.nivel(x.id).picoDb)} />
                <Fader valor={x.fader} mudo={x.mudo} rotulo={t('tv.som.faderDe', { nome: x.nome })} onChange={(v) => som.mudar(x.id, { fader: v })} />
              </div>
              <span className="tv-faders__nome">{x.nome}</span>
            </div>
          ))}
          <div className="tv-faders__f is-mestre">
            <div className="tv-tira__fader">
              <Medidor ler={() => alturaDoMedidor(Math.max(som.nivelMestre()[0].picoDb, som.nivelMestre()[1].picoDb))} />
              <Fader valor={som.faderMestre} activo rotulo={t('tv.som.faderMestre')} onChange={(v) => som.mudarMestre(v)} />
            </div>
            <span className="tv-faders__nome">{t('tv.cena.mistura')}</span>
          </div>
        </div>
      )}
    </Cartao>
  )
}

/** O alinhamento do programa, guardado neste dispositivo até o servidor o guardar. */
function Alinhamento() {
  const { t } = useTranslation()
  const [estado, setEstado] = useState(lerAlinhamento)
  const [desde, setDesde] = useState(0)
  const [titulo, setTitulo] = useState('')
  const [minutos, setMinutos] = useState('')
  const [aEscrever, setAEscrever] = useState(false)
  const guardar = (e: typeof estado) => {
    setEstado(e)
    try {
      localStorage.setItem(CHAVE_ALINHAMENTO, JSON.stringify(e))
    } catch {
      /* fica para esta sessão */
    }
  }
  const adicionar = (ev: FormEvent) => {
    ev.preventDefault()
    const nome = titulo.trim()
    if (!nome) return
    const mins = Number(minutos.replace(',', '.'))
    guardar({ ...estado, itens: [...estado.itens, { titulo: nome.slice(0, 80), duracaoS: Number.isFinite(mins) ? Math.round(mins * 60) : 0, nota: '' }] })
    setTitulo('')
    setMinutos('')
    setAEscrever(false)
  }
  const total = estado.itens.reduce((a, b) => a + b.duracaoS, 0)
  return (
    <>
      <Cabeca>
        <h2 className="tv-t1">{t('tv.cena.alinhamento')}</h2>
        {estado.itens.length > 0 && <span className="tv-mono-85">{duracao(total)}</span>}
        <BotaoTv aDireita mini onClick={() => setAEscrever((v) => !v)}>
          {t('tv.cena.adicionar')}
        </BotaoTv>
      </Cabeca>
      {aEscrever && (
        <form className="tv-form-item" onSubmit={adicionar} data-typing>
          <input value={titulo} onChange={(e) => setTitulo(e.target.value)} placeholder={t('tv.cena.itemTitulo')} aria-label={t('tv.cena.itemTitulo')} maxLength={80} />
          <input value={minutos} onChange={(e) => setMinutos(e.target.value)} placeholder={t('tv.cena.itemMinutos')} aria-label={t('tv.cena.itemMinutos')} inputMode="decimal" />
          <BotaoTv mini variante="accent" type="submit" disabled={!titulo.trim()}>
            {t('tv.cena.guardarItem')}
          </BotaoTv>
        </form>
      )}
      {estado.itens.length === 0 ? (
        <p className="tv-nota">{t('tv.cena.alinhamentoVazio')}</p>
      ) : (
        <ol className="tv-alinhamento" data-tv="alinhamento">
          {estado.itens.map((it, i) => (
            <li key={i} className={cx('tv-item', i === estado.actual && 'is-live', i === estado.actual + 1 && 'is-next')}>
              <span className="tv-item__linha">
                <span className="tv-item__num">{String(i + 1).padStart(2, '0')}</span>
                <span className="tv-item__nome">{it.titulo}</span>
                <span className="tv-item__dur">
                  {i === estado.actual && desde ? <Desde desde={desde} /> : null}
                  {i === estado.actual && desde ? ' / ' : ''}
                  {it.duracaoS ? duracao(it.duracaoS) : ''}
                </span>
              </span>
              {it.nota && <span className="tv-item__nota">{it.nota}</span>}
            </li>
          ))}
        </ol>
      )}
      {estado.itens.length > 0 && (
        <Cabeca>
          <BotaoTv
            mini
            variante="accent"
            disabled={estado.actual >= estado.itens.length - 1}
            onClick={() => {
              guardar({ ...estado, actual: estado.actual + 1 })
              setDesde(Date.now())
            }}
          >
            {estado.actual < 0 ? t('tv.cena.comecar') : t('tv.cena.seguinte')}
          </BotaoTv>
          {estado.actual >= 0 && (
            <BotaoTv mini onClick={() => (guardar({ ...estado, actual: -1 }), setDesde(0))}>
              {t('tv.cena.reiniciar')}
            </BotaoTv>
          )}
          <span className="tv-dir tv-mono-85">{t('tv.cena.noDispositivo')}</span>
        </Cabeca>
      )}
    </>
  )
}
