/**
 * DelonixSwitcher — a mesa de corte: pré-visualização e programa, o
 * barramento de seis fontes com tally, as quatro transições com duração, a
 * T-bar e o AUTO, as sobreposições, as macros F1–F6, o corte automático por
 * voz e os atalhos.
 */
import { PointerEvent as ReactPointerEvent, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { teclaDeSobreposicao } from '../../../studio/tv/atalhos'
import { SOBREPOSICOES_DA_MESA } from '../../../studio/tv/macros'
import { fontePrincipal, planoDe, TRANSICOES, type TipoDeTransicao } from '../../../studio/tv/mesa'
import { alturaDoMedidor } from '../../../studio/tv/som'
import { cx } from '../../../ui/kit'
import { CartaoDoBarramento, LugarVazio, type PropsDoEcra, SelosDoAr, useNomeDaFonte } from './comum'
import { BarrasDeNivel, Desde, Deslizador, Espaco, Interruptor, MonitorDePlano, MonitorDoPrograma, TopoTv, useTique } from './pecas'

const TECLA_DA_TRANSICAO: Record<TipoDeTransicao, string> = { cortar: 'espaco', misturar: 'enter', limpar: 'W', stinger: 'S' }

export default function MesaDeCorte({ s, c }: PropsDoEcra) {
  const { t, i18n } = useTranslation()
  const nome = useNomeDaFonte(s)
  const bus = s.registo.barramento()
  const m = s.mesa
  const noAr = c.directo.fase === 'no-ar'
  const mod = teclaDeSobreposicao()
  const idPre = fontePrincipal(m.previa)
  const idPgm = fontePrincipal(m.emCurso ? m.emCurso.para : m.programa)
  const nivelDoPrograma = () => {
    if (!s.som) return 0
    const [l, r] = s.som.nivelMestre()
    return alturaDoMedidor(Math.max(l.picoDb, r.picoDb))
  }

  return (
    <div className="dx-stage tv tv--palco" data-tv-ecra="mesa-de-corte">
      <TopoTv titulo={t('tv.corte.titulo')} onVoltar={() => c.onNavegar(null)}>
        <SelosDoAr c={c} />
        <Espaco />
        {noAr && c.kbps > 0 && (
          <span className="tv-meta" data-tv="bitrate">
            {t('tv.topo.bitrate', { mbps: (c.kbps / 1000).toLocaleString(i18n.language, { maximumFractionDigits: 1, minimumFractionDigits: 1 }) })}
          </span>
        )}
        <button type="button" className="tv-botao" onClick={() => c.onNavegar('mesa-de-som')}>
          {t('tv.nav.mesaDeSom')}
        </button>
        <button type="button" className="tv-botao" onClick={() => c.onNavegar('iluminacao')}>
          {t('tv.nav.iluminacao')}
        </button>
      </TopoTv>

      <div className="tv-corpo tv-corte">
        <section className="tv-centro" aria-label={t('tv.corte.rotulo')}>
          <div className="tv-par">
            <div className="tv-monitor tv-monitor--pre">
              <div className="tv-monitor__cabeca">
                <span className="tv-monitor__estado">{t('tv.corte.previa')}</span>
                <span className="tv-monitor__nome" data-tv="nome-previa">
                  {idPre ? nome(idPre) : t('tv.corte.semPrevia')}
                </span>
                <span className="tv-monitor__lado">{t('tv.corte.proximo')}</span>
              </div>
              <div className="tv-monitor__corpo">
                <MonitorDePlano fontes={s.registo} plano={m.previa} />
                {!m.previa && <span className="tv-monitor__vazio">{t('tv.corte.semSinal')}</span>}
              </div>
            </div>
            <div className="tv-monitor tv-monitor--pgm">
              <div className="tv-monitor__cabeca">
                <span className="tv-monitor__estado">{noAr ? t('tv.corte.programaNoAr') : t('tv.corte.programa')}</span>
                <span className="tv-monitor__nome" data-tv="nome-programa">
                  {idPgm ? nome(idPgm) : t('tv.corte.palcoDoEstudio')}
                </span>
                <span className="tv-monitor__lado">{m.programa && <Desde desde={m.noArDesde} />}</span>
              </div>
              <div className="tv-monitor__corpo">
                <MonitorDoPrograma compRef={c.compRef} devolverA={c.canvasHostRef} />
                <span className="tv-monitor__sobre">
                  {noAr && <span className="tv-monitor__rotulo">{t('tv.corte.marcaAoVivo')}</span>}
                </span>
                <span className="tv-monitor__rodape">
                  {c.titulo.trim() && <span className="tv-monitor__titulo">{c.titulo.trim()}</span>}
                  {s.som && <BarrasDeNivel ler={nivelDoPrograma} />}
                </span>
              </div>
            </div>
          </div>

          <ul className="tv-bus" aria-label={t('tv.corte.barramento')}>
            {Array.from({ length: 6 }, (_, i) => {
              const f = bus[i]
              return (
                <li key={f?.id ?? `vazio-${i}`}>
                  {f ? (
                    <CartaoDoBarramento
                      s={s}
                      fonte={f}
                      numero={i + 1}
                      onPrevia={() => s.accoes.previa(planoDe(f.id))}
                      onAr={() => s.accoes.ar(planoDe(f.id))}
                    />
                  ) : (
                    <LugarVazio numero={i + 1} onAbrir={() => c.onNavegar('fontes')} />
                  )}
                </li>
              )
            })}
          </ul>

          <div className="tv-baixo">
            <section className="tv-cartao tv-cartao--painel" aria-labelledby="tv-trans-h">
              <div className="tv-cabeca">
                <h2 id="tv-trans-h" className="tv-t1">
                  {t('tv.corte.transicao')}
                </h2>
                <span className="tv-mono-9">{t('tv.corte.dicaTransicao')}</span>
              </div>
              <div className="tv-trans" role="group" aria-label={t('tv.corte.transicao')}>
                {TRANSICOES.map((tipo) => (
                  <button
                    key={tipo}
                    type="button"
                    className="tv-trans__b"
                    aria-pressed={m.transicao === tipo}
                    data-tv={`transicao-${tipo}`}
                    disabled={!m.previa}
                    onClick={() => s.accoes.escolher(tipo)}
                  >
                    <strong>{t(`tv.transicoes.${tipo}`)}</strong>
                    <span>{TECLA_DA_TRANSICAO[tipo] === 'espaco' ? t('tv.teclas.espaco') : TECLA_DA_TRANSICAO[tipo] === 'enter' ? t('tv.teclas.enter') : TECLA_DA_TRANSICAO[tipo]}</span>
                  </button>
                ))}
              </div>
              <Deslizador
                rotulo={t('tv.corte.duracao')}
                valor={m.duracaoMs}
                min={100}
                max={3000}
                passo={100}
                texto={t('tv.unidades.segundos', { v: (m.duracaoMs / 1000).toLocaleString(i18n.language, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}
                onChange={s.accoes.duracao}
                larguraValor={58}
                data-tv="duracao"
              />
              <div className="tv-divisor" />
              <div className="tv-cabeca">
                <h2 className="tv-t1">{t('tv.corte.sobreposicoes')}</h2>
                <span className="tv-mono-9">{t('tv.corte.chavesAJusante')}</span>
              </div>
              <div className="tv-keys">
                {SOBREPOSICOES_DA_MESA.map((q, i) => {
                  const semSondagem = q === 'sondagem' && !c.haSondagem
                  const semTexto = q === 'legenda' && !c.palco.sobreposicoes.nome.trim() && !c.palco.sobreposicoes.cargo.trim()
                  return (
                    <button
                      key={q}
                      type="button"
                      className="tv-key"
                      aria-pressed={s.sobreposicaoLigada(q)}
                      disabled={semSondagem}
                      title={semSondagem ? t('tv.corte.semSondagem') : semTexto ? t('tv.corte.legendaSemTexto') : undefined}
                      data-tv={`sobreposicao-${q}`}
                      onClick={() => s.definirSobreposicao(q, !s.sobreposicaoLigada(q))}
                    >
                      <span>{t(`tv.sobreposicoes.${q}`)}</span>
                      <span>{`${mod}${i + 1}`}</span>
                    </button>
                  )
                })}
              </div>
            </section>

            <TBar s={s} />
          </div>
        </section>

        <aside className="tv-col tv-col--dir" aria-label={t('tv.corte.macros')}>
          <div className="tv-cabeca">
            <h2 className="tv-t1">{t('tv.corte.macros')}</h2>
            <span className="tv-dir tv-mono-85">{t('tv.corte.macrosTeclas')}</span>
          </div>
          <div className="tv-rolar" style={{ display: 'flex', flexDirection: 'column', gap: 11 }}>
            {s.macros.map((macro) => {
              const p = s.progressoMacro?.tecla === macro.tecla ? s.progressoMacro : null
              const aCorrer = !!p && p.indice < p.total
              return (
                <button
                  key={macro.tecla}
                  type="button"
                  className={cx('tv-macro', aCorrer && 'is-activa')}
                  data-tv={`macro-f${macro.tecla}`}
                  onClick={() => s.correrMacroDaTecla(macro.tecla)}
                >
                  <span className="tv-cabeca">
                    <span className="tv-macro__nome">{t(`tv.macros.${macro.id}.nome`)}</span>
                    <span className="tv-dir tv-mono-85">{`F${macro.tecla}`}</span>
                  </span>
                  <span className="tv-nota" style={{ fontSize: 9 }}>
                    {t(`tv.macros.${macro.id}.nota`)}
                  </span>
                  {p && (
                    <span className="tv-macro__passos" aria-label={t('tv.corte.progressoMacro', { feitos: p.resultados.length, total: p.total })}>
                      {macro.passos.map((_, i) => {
                        const r = p.resultados[i]
                        return (
                          <span
                            key={i}
                            className={cx('tv-macro__passo', r?.estado === 'feito' && 'is-feito', r?.estado === 'indisponivel' && 'is-falhou')}
                            title={r?.estado === 'indisponivel' ? t(`tv.macros.razoes.${r.razao}`) : undefined}
                          />
                        )
                      })}
                    </span>
                  )}
                  {p && !aCorrer && p.resultados.some((r) => r.estado === 'indisponivel') && (
                    <span className="tv-mono-85 tv-aviso" data-tv="macro-aviso">
                      {p.resultados
                        .filter((r): r is { estado: 'indisponivel'; razao: string } => r.estado === 'indisponivel')
                        .map((r) => t(`tv.macros.razoes.${r.razao}`))
                        .join(' · ')}
                    </span>
                  )}
                </button>
              )
            })}
          </div>

          <div className="tv-cartao" style={{ background: 'var(--raised)', padding: 9, gap: 6 }}>
            <h2 className="tv-t3">{t('tv.corte.vozTitulo')}</h2>
            <div className="tv-cabeca">
              <Interruptor
                ligado={s.vozLigada}
                rotulo={t('tv.corte.vozTitulo')}
                disabled={!s.som || s.canaisComFonte === 0}
                onChange={s.setVozLigada}
              />
              <span className="tv-nota" data-tv="voz-estado">
                {!s.som
                  ? t('tv.corte.vozSemMesaDeSom')
                  : s.canaisComFonte === 0
                    ? t('tv.corte.vozSemCanais')
                    : s.vozLigada
                      ? t('tv.corte.vozLigado', { count: s.canaisComFonte })
                      : t('tv.corte.vozDesligado')}
              </span>
            </div>
          </div>

          <div className="tv-cartao tv-cartao--nota" style={{ marginTop: 'auto', padding: 9 }}>
            <p className="tv-eyebrow">{t('tv.corte.atalhos')}</p>
            <dl className="tv-atalhos">
              <div>
                <dt>{t('tv.atalhos.teclas.previa')}</dt>
                <dd>{t('tv.atalhos.previa')}</dd>
              </div>
              <div>
                <dt>{t('tv.atalhos.teclas.ar')}</dt>
                <dd>{t('tv.atalhos.ar')}</dd>
              </div>
              <div>
                <dt>{t('tv.teclas.espaco')}</dt>
                <dd>{t('tv.atalhos.cortar')}</dd>
              </div>
              <div>
                <dt>{t('tv.teclas.enter')}</dt>
                <dd>{t('tv.atalhos.misturar')}</dd>
              </div>
              <div>
                <dt>{t('tv.atalhos.teclas.sobreposicoes', { mod })}</dt>
                <dd>{t('tv.atalhos.sobreposicoes')}</dd>
              </div>
            </dl>
          </div>
        </aside>
      </div>
    </div>
  )
}

/** A T-bar: arrastar faz a transição escolhida à mão; CORTAR e AUTO por baixo. */
function TBar({ s }: { s: PropsDoEcra['s'] }) {
  const { t } = useTranslation()
  const pista = useRef<HTMLDivElement>(null)
  const pega = useRef<HTMLSpanElement>(null)
  const enchimento = useRef<HTMLSpanElement>(null)
  const pct = useRef<HTMLSpanElement>(null)

  // A barra desenha-se do estado em ref: um AUTO move-a sem re-render por frame.
  useTique(() => {
    const e = s.mesaRef.current
    let pos = e.tbar
    let prog = 0
    if (e.emCurso) {
      prog = e.emCurso.inicio < 0 ? (e.tbarEmBaixo ? 1 - e.tbar : e.tbar) : Math.min(1, (Date.now() - e.emCurso.inicio) / e.emCurso.duracaoMs)
      if (e.emCurso.inicio >= 0) pos = e.tbarEmBaixo ? 1 - prog : prog
    }
    if (pega.current) pega.current.style.bottom = `${pos * 100}%`
    if (enchimento.current) enchimento.current.style.height = `${pos * 100}%`
    if (pct.current) pct.current.textContent = `${Math.round(prog * 100)}%`
  })

  const arrastar = (e: ReactPointerEvent<HTMLDivElement>) => {
    const el = pista.current
    if (!el || !s.mesa.previa) return
    el.setPointerCapture(e.pointerId)
    const mover = (ev: PointerEvent) => {
      const r = el.getBoundingClientRect()
      s.accoes.tbar(1 - Math.min(1, Math.max(0, (ev.clientY - r.top) / r.height)))
    }
    mover(e.nativeEvent)
    const largar = () => {
      el.removeEventListener('pointermove', mover)
      el.removeEventListener('pointerup', largar)
      el.removeEventListener('pointercancel', largar)
    }
    el.addEventListener('pointermove', mover)
    el.addEventListener('pointerup', largar)
    el.addEventListener('pointercancel', largar)
  }

  const teclado = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const passo = e.key === 'ArrowUp' ? 0.05 : e.key === 'ArrowDown' ? -0.05 : 0
    if (!passo) return
    e.preventDefault()
    e.stopPropagation()
    s.accoes.tbar(s.mesaRef.current.tbar + passo)
  }

  return (
    <div className="tv-tbar">
      <span className="tv-mono-85" style={{ letterSpacing: 0.6 }}>
        {t('tv.corte.tbar')}
      </span>
      <div
        ref={pista}
        className="tv-tbar__pista"
        role="slider"
        tabIndex={0}
        aria-label={t('tv.corte.tbarAria')}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(s.mesa.tbar * 100)}
        aria-disabled={!s.mesa.previa}
        data-tv="tbar"
        onPointerDown={arrastar}
        onKeyDown={teclado}
      >
        <span ref={enchimento} className="tv-tbar__enchimento" />
        <span ref={pega} className="tv-tbar__pega" />
        <span className="tv-tbar__fim tv-tbar__fim--pre">{t('tv.tally.previa')}</span>
        <span className="tv-tbar__fim tv-tbar__fim--pgm">{t('tv.tally.programa')}</span>
      </div>
      <span ref={pct} className="dx-num" style={{ fontSize: 10 }} data-tv="tbar-pct" />
      <button type="button" className="tv-tbar__botao" data-tv="cortar" disabled={!s.mesa.previa} onClick={s.accoes.cortar}>
        {t('tv.transicoes.cortar')}
      </button>
      <button type="button" className="tv-tbar__botao tv-tbar__botao--auto" data-tv="auto" disabled={!s.mesa.previa} onClick={() => s.accoes.auto()}>
        {t('tv.corte.auto')}
      </button>
    </div>
  )
}
