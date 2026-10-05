/**
 * DelonixAudioMixer — a mesa de som: uma tira por canal (ganho, medidor,
 * fader, mudo, solo), a mistura PGM, os barramentos com o retorno de
 * auscultadores, e o inspector do canal escolhido (EQ de 4 bandas com o
 * espectro medido, dinâmica, limpeza automática).
 */
import { CSSProperties, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { curvaDoEq, EQ_GANHO_MAXIMO, alturaDoMedidor, faderParaDb, formatarDb, type LeituraDeSonoridade } from '../../../studio/tv/som'
import type { EstadoDoCanal } from '../../../studio/tv/mesaDeSom'
import { Icon, type IconName } from '../../../ui/icons'
import { cx } from '../../../ui/kit'
import { type PropsDoEcra, SelosDoAr } from './comum'
import { BotaoTv, Cabeca, Cartao, Deslizador, Espaco, Fader, Interruptor, Medidor, TopoTv, useTique } from './pecas'

const ICONE: Record<EstadoDoCanal['tipo'], IconName> = { microfone: 'mic', participante: 'people', ecra: 'screen' }
export const ALVO_LUFS = -16

export default function MesaDeSomEcra({ s, c }: PropsDoEcra) {
  const { t, i18n } = useTranslation()
  const som = s.som
  const [escolhido, setEscolhido] = useState('')
  const [guardada, setGuardada] = useState('')
  const canais = som?.lista() ?? []
  const canal = canais.find((x) => x.id === escolhido) ?? canais[0] ?? null
  const noAr = c.directo.fase === 'no-ar'
  const lang = i18n.language

  // A mesa liga-se à primeira visita (é um clique: o AudioContext pode arrancar).
  useEffect(() => {
    if (!som) void s.ligarSom()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const microfonesLivres = s.microfones.filter((m) => !canais.some((x) => x.id === `mic:${m.deviceId || 'omissao'}`))
  const info = som?.info()

  return (
    <div className="dx-stage tv" data-tv-ecra="mesa-de-som">
      <TopoTv titulo={t('tv.som.titulo')} onVoltar={() => c.onNavegar(null)}>
        <span className="tv-chip" data-tv="canais">
          {t('tv.som.contagem', { count: canais.length })}
        </span>
        <SelosDoAr c={c} destinosNoSelo={false} />
        <Espaco />
        {som?.temSonoridade && <Sonoridade s={s} />}
        <BotaoTv
          disabled={!som || !canais.length}
          data-tv="guardar-cena-som"
          onClick={() => {
            const n = som?.guardarCena() ?? 0
            setGuardada(n ? t('tv.som.cenaGuardada', { count: n }) : t('tv.som.cenaNaoGuardada'))
            window.setTimeout(() => setGuardada(''), 4000)
          }}
        >
          {t('tv.som.guardarCena')}
        </BotaoTv>
        <BotaoTv variante="forte" onClick={() => c.onNavegar('mesa-de-corte')}>
          {t('tv.nav.mesaDeCorte')}
        </BotaoTv>
      </TopoTv>

      <div className="tv-corpo tv-som">
        <section className="tv-centro" style={{ gap: 11 }} aria-label={t('tv.som.canais')}>
          <div className="tv-camadas">
            <span className="tv-camada is-on">{t('tv.som.canaisN', { count: canais.length })}</span>
            {som && microfonesLivres.length > 0 && (
              <label className="tv-camada">
                <select
                  aria-label={t('tv.som.adicionarMicrofone')}
                  value=""
                  data-tv="adicionar-microfone"
                  onChange={(e) => {
                    const m = microfonesLivres.find((x) => x.deviceId === e.target.value)
                    if (m) void som.adicionarMicrofone(m.deviceId, m.nome).catch(() => undefined)
                  }}
                >
                  <option value="">{t('tv.som.adicionarMicrofone')}</option>
                  {microfonesLivres.map((m) => (
                    <option key={m.deviceId} value={m.deviceId}>
                      {m.nome}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {guardada && (
              <span className="tv-mono-9 tv-ok" role="status">
                {guardada}
              </span>
            )}
            {info && (
              <span className="tv-dir tv-mono-9" data-tv="formato">
                {[
                  t('tv.som.taxa', { khz: (info.taxa / 1000).toLocaleString(lang, { maximumFractionDigits: 1 }) }),
                  info.bits ? t('tv.som.bits', { bits: info.bits }) : null,
                  t('tv.som.latencia', { ms: Math.round(info.latenciaMs) }),
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            )}
          </div>

          {!som ? (
            <div className="tv-vazio">
              {s.erroSom ? <p className="tv-erro">{s.erroSom}</p> : <span>{t('tv.som.aLigar')}</span>}
            </div>
          ) : (
            <div className="tv-tiras" data-tv="tiras">
              {canais.length === 0 && (
                <div className="tv-vazio">
                  <strong>{t('tv.som.semCanais')}</strong>
                  <span>{t('tv.som.semCanaisDica')}</span>
                </div>
              )}
              {canais.map((x, i) => (
                <Tira key={x.id} s={s} canal={x} n={i + 1} escolhido={canal?.id === x.id} onEscolher={() => setEscolhido(x.id)} />
              ))}
              <span className="tv-sep" />
              <div className="tv-mestre" data-tv="mestre">
                <span className="tv-mono-85 tv-accent" style={{ fontSize: 8 }}>
                  {t('tv.tally.programa')}
                </span>
                <span style={{ fontSize: 9.5, fontWeight: 700, textAlign: 'center', lineHeight: 1.2 }}>{t('tv.som.misturaAoAr')}</span>
                <div className="tv-tira__fader">
                  <Medidor ler={() => alturaDoMedidor(som.nivelMestre()[0].picoDb)} />
                  <Medidor ler={() => alturaDoMedidor(som.nivelMestre()[1].picoDb)} />
                  <Fader largo valor={som.faderMestre} rotulo={t('tv.som.faderMestre')} onChange={(v) => som.mudarMestre(v)} />
                </div>
                <span className="dx-num tv-accent" style={{ fontSize: 8.5 }}>
                  {t('tv.unidades.db', { v: formatarDb(faderParaDb(som.faderMestre), lang) })}
                </span>
                <span className={cx('tv-mestre__ar', !noAr && 'is-off')}>{noAr ? t('tv.som.aoAr') : t('tv.som.foraDoAr')}</span>
              </div>
            </div>
          )}

          {som && (
            <div className="tv-barramentos" data-tv="barramentos">
              <span className="tv-t3">{t('tv.som.barramentos')}</span>
              <span className="tv-bus-chip">
                <b>{t('tv.som.aux1')}</b>
                <span>{t('tv.som.retornoAuscultadores')}</span>
                <span className="dx-num" style={{ color: 'var(--text)' }}>
                  {t('tv.unidades.db', { v: formatarDb(faderParaDb(som.faderAux), lang) })}
                </span>
                {som.podeEscolherSaida && s.saidas.length > 1 && (
                  <select aria-label={t('tv.som.saidaDoRetorno')} onChange={(e) => void som.ligarAux(som.auxLigado, e.target.value)}>
                    {s.saidas.map((o) => (
                      <option key={o.deviceId} value={o.deviceId}>
                        {o.nome}
                      </option>
                    ))}
                  </select>
                )}
              </span>
              <span className="tv-bus-chip" title={t('tv.aguardarServidor')}>
                <b>{t('tv.som.grav')}</b>
                <span>{t('tv.som.faixasIso')}</span>
                <span className="tv-mono-85">{t('tv.aguardarServidorCurto')}</span>
              </span>
              <span className="tv-dir tv-mono-9">{t('tv.som.retornoDoEstudio')}</span>
              <button
                type="button"
                className="tv-retorno"
                aria-pressed={som.auxLigado}
                data-tv="retorno"
                onClick={() => void som.ligarAux(!som.auxLigado)}
              >
                {som.auxLigado ? t('tv.som.auscultadoresDb', { v: formatarDb(faderParaDb(som.faderAux), lang, 0) }) : t('tv.som.auscultadoresDesligados')}
              </button>
            </div>
          )}
        </section>

        <aside className="tv-col tv-col--dir" aria-label={t('tv.som.inspector')}>
          {som && canal ? <Inspector s={s} canal={canal} n={canais.indexOf(canal) + 1} /> : <div className="tv-vazio">{t('tv.som.escolheCanal')}</div>}
        </aside>
      </div>
    </div>
  )
}

function Sonoridade({ s }: { s: PropsDoEcra['s'] }) {
  const { t, i18n } = useTranslation()
  const valor = useRef<HTMLSpanElement>(null)
  const detalhe = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (!s.som || agora - ultimo.current < 250) return
    ultimo.current = agora
    const l: LeituraDeSonoridade = s.som.sonoridadeAgora()
    const v = Number.isFinite(l.curta) ? l.curta : l.momentanea
    if (valor.current) {
      valor.current.textContent = Number.isFinite(v) ? formatarDb(v, i18n.language) : '—'
      valor.current.className = cx('dx-num', Math.abs(v - ALVO_LUFS) <= 1 ? 'tv-ok' : Number.isFinite(v) ? 'tv-aviso' : 'tv-muted')
      valor.current.dataset.lufs = Number.isFinite(v) ? v.toFixed(2) : ''
    }
    if (detalhe.current) {
      detalhe.current.textContent = t('tv.som.alvoEPico', {
        alvo: formatarDb(ALVO_LUFS, i18n.language, 0),
        pico: Number.isFinite(l.picoRealRecente) ? formatarDb(l.picoRealRecente, i18n.language) : '—',
      })
    }
  })
  return (
    <span className="tv-chip" style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 10px' }} data-tv="lufs">
      <span style={{ fontSize: 9 }}>{t('tv.som.lufs')}</span>
      <span ref={valor} className="dx-num" style={{ fontSize: 11, fontWeight: 500 }} />
      <span ref={detalhe} style={{ fontSize: 9 }} />
    </span>
  )
}

function Tira({ s, canal, n, escolhido, onEscolher }: { s: PropsDoEcra['s']; canal: EstadoDoCanal; n: number; escolhido: boolean; onEscolher: () => void }) {
  const { t, i18n } = useTranslation()
  const som = s.som!
  const rot = -130 + (canal.ganhoDb / 20) * 130
  return (
    <div
      className={cx('tv-tira', escolhido && 'is-sel', canal.mudo && 'is-mudo')}
      data-tv="tira"
      data-canal={canal.id}
      onClick={onEscolher}
      role="group"
      aria-label={t('tv.som.tiraAria', { n, nome: canal.nome })}
    >
      <span className="tv-mono-85" style={{ fontSize: 8 }}>
        {String(n).padStart(2, '0')}
      </span>
      <span className="tv-tira__icone">
        <Icon name={ICONE[canal.tipo]} />
      </span>
      <span className="tv-tira__nome">{canal.nome}</span>
      <span className="tv-botao-knob" style={{ '--rot': `${rot}deg` } as CSSProperties}>
        <input
          type="range"
          min={-20}
          max={20}
          step={0.5}
          value={canal.ganhoDb}
          aria-label={t('tv.som.ganhoDe', { nome: canal.nome })}
          onChange={(e) => som.mudar(canal.id, { ganhoDb: Number(e.target.value) })}
        />
      </span>
      <span className="tv-mono-85" style={{ fontSize: 8 }}>
        {t('tv.unidades.db', { v: formatarDb(canal.ganhoDb, i18n.language) })}
      </span>
      <div className="tv-tira__fader">
        <Medidor ler={() => alturaDoMedidor(som.nivel(canal.id).picoDb)} />
        <Fader valor={canal.fader} activo={escolhido} mudo={canal.mudo} rotulo={t('tv.som.faderDe', { nome: canal.nome })} onChange={(v) => som.mudar(canal.id, { fader: v })} />
      </div>
      <span className={cx('dx-num', canal.mudo && 'tv-muted')} style={{ fontSize: 8 }} data-tv="fader-db">
        {formatarDb(faderParaDb(canal.fader), i18n.language)}
      </span>
      <span className="tv-tira__botoes">
        <button
          type="button"
          className="tv-ms tv-ms--mudo"
          aria-pressed={canal.mudo}
          aria-label={t('tv.som.mudoDe', { nome: canal.nome })}
          data-tv="mudo"
          onClick={(e) => {
            e.stopPropagation()
            som.mudar(canal.id, { mudo: !canal.mudo })
          }}
        >
          {t('tv.som.m')}
        </button>
        <button
          type="button"
          className="tv-ms tv-ms--solo"
          aria-pressed={canal.solo}
          aria-label={t('tv.som.soloDe', { nome: canal.nome })}
          title={t('tv.som.soloDica')}
          onClick={(e) => {
            e.stopPropagation()
            som.mudar(canal.id, { solo: !canal.solo })
          }}
        >
          {t('tv.som.s')}
        </button>
      </span>
    </div>
  )
}

function Inspector({ s, canal, n }: { s: PropsDoEcra['s']; canal: EstadoDoCanal; n: number }) {
  const { t, i18n } = useTranslation()
  const som = s.som!
  const lang = i18n.language
  const d = canal.dinamica
  const mudarDin = (patch: Partial<typeof d>) => som.mudar(canal.id, { dinamica: { ...d, ...patch } })
  const reducao = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (agora - ultimo.current < 150 || !reducao.current) return
    ultimo.current = agora
    const r = som.reducao(canal.id)
    reducao.current.textContent = r < -0.1 ? t('tv.som.aReduzir', { db: formatarDb(-r, lang) }) : t('tv.som.semReducao')
  })
  const eMic = canal.tipo === 'microfone'
  const fontes = s.registo.lista()

  return (
    <>
      <Cabeca>
        <span style={{ width: 8, height: 8, borderRadius: 'var(--r-2)', background: 'var(--accent)', flex: 'none' }} />
        <h2 className="tv-t1" style={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
          {t('tv.som.canalN', { n, nome: canal.nome })}
        </h2>
        <span className="tv-dir tv-tag tv-tag--forte">{t(`tv.som.tipos.${canal.tipo}`)}</span>
      </Cabeca>

      <label className="tv-seleccao">
        <span>{t('tv.som.fonteDoCanal')}</span>
        <select value={canal.fonte ?? ''} onChange={(e) => som.mudar(canal.id, { fonte: e.target.value || null })} data-tv="fonte-do-canal">
          <option value="">{t('tv.som.semFonte')}</option>
          {fontes.map((f) => (
            <option key={f.id} value={f.id}>
              {f.nome}
            </option>
          ))}
        </select>
      </label>

      <Cartao como="section" aria-label={t('tv.som.equalizador')}>
        <Cabeca>
          <span className="tv-mono-85">{t('tv.som.equalizador')}</span>
          <Cabeca como="span" className="tv-dir" style={{ gap: 6 }}>
            <span className={cx('tv-mono-85', canal.eqLigado && 'tv-ok')}>{canal.eqLigado ? t('tv.estado.ligado') : t('tv.estado.desligado')}</span>
            <Interruptor pequeno ligado={canal.eqLigado} rotulo={t('tv.som.equalizador')} onChange={(v) => som.mudar(canal.id, { eqLigado: v })} />
          </Cabeca>
        </Cabeca>
        <Espectro s={s} canal={canal} />
        <div className="tv-bandas">
          {canal.eq.map((b, i) => (
            <label key={i} className="tv-banda" data-tv={`banda-${i}`}>
              <span>{t(`tv.som.bandas.${i}`)}</span>
              <span>{formatarDb(b.ganhoDb, lang)}</span>
              <span>{b.freq >= 1000 ? t('tv.unidades.khz', { v: (b.freq / 1000).toLocaleString(lang, { maximumFractionDigits: 1 }) }) : t('tv.unidades.hz', { v: b.freq })}</span>
              <input
                type="range"
                min={-EQ_GANHO_MAXIMO}
                max={EQ_GANHO_MAXIMO}
                step={0.5}
                value={b.ganhoDb}
                aria-label={t('tv.som.bandaAria', { banda: t(`tv.som.bandas.${i}`) })}
                onChange={(e) => som.mudar(canal.id, { eq: canal.eq.map((x, j) => (j === i ? { ...x, ganhoDb: Number(e.target.value) } : x)) })}
              />
            </label>
          ))}
        </div>
      </Cartao>

      <Cartao como="section" style={{ gap: 9 }} aria-label={t('tv.som.dinamica')}>
        <Cabeca>
          <span className="tv-mono-85">{t('tv.som.dinamica')}</span>
          <span ref={reducao} className="tv-dir tv-mono-85 tv-ok" data-tv="reducao" />
        </Cabeca>
        <Deslizador
          rotulo={t('tv.som.porta')}
          valor={d.porta.ligada ? d.porta.limiarDb : -80}
          min={-80}
          max={-10}
          disabled={!som.temPorta}
          texto={d.porta.ligada ? t('tv.unidades.db', { v: formatarDb(d.porta.limiarDb, lang, 0) }) : t('tv.estado.desligado')}
          onChange={(v) => mudarDin({ porta: { ligada: v > -80, limiarDb: v } })}
          larguraRotulo={76}
        />
        <Deslizador
          rotulo={t('tv.som.compressor')}
          valor={d.compressor.ligado ? d.compressor.razao : 1}
          min={1}
          max={20}
          passo={0.1}
          texto={d.compressor.ligado ? t('tv.unidades.razao', { v: d.compressor.razao.toLocaleString(lang, { maximumFractionDigits: 1 }) }) : t('tv.estado.desligado')}
          onChange={(v) => mudarDin({ compressor: { ...d.compressor, ligado: v > 1, razao: v } })}
          larguraRotulo={76}
          data-tv="compressor"
        />
        <Deslizador
          rotulo={t('tv.som.ataque')}
          valor={d.compressor.ataqueMs}
          min={1}
          max={100}
          texto={t('tv.unidades.ms', { v: d.compressor.ataqueMs })}
          onChange={(v) => mudarDin({ compressor: { ...d.compressor, ataqueMs: v } })}
          larguraRotulo={76}
        />
        <Deslizador
          rotulo={t('tv.som.recuperacao')}
          valor={d.compressor.recuperacaoMs}
          min={20}
          max={1000}
          passo={10}
          texto={t('tv.unidades.ms', { v: d.compressor.recuperacaoMs })}
          onChange={(v) => mudarDin({ compressor: { ...d.compressor, recuperacaoMs: v } })}
          larguraRotulo={76}
        />
        <Deslizador
          rotulo={t('tv.som.limitador')}
          valor={d.limitador.ligado ? d.limitador.tectoDb : 0}
          min={-12}
          max={0}
          passo={0.5}
          texto={d.limitador.ligado ? t('tv.unidades.db', { v: formatarDb(d.limitador.tectoDb, lang) }) : t('tv.estado.desligado')}
          onChange={(v) => mudarDin({ limitador: { ligado: v < 0, tectoDb: v } })}
          larguraRotulo={76}
        />
      </Cartao>

      <Cartao como="section" style={{ gap: 7 }} aria-label={t('tv.som.limpeza')}>
        <h3 className="tv-t2">{t('tv.som.limpeza')}</h3>
        <button
          type="button"
          className="tv-limpeza"
          aria-pressed={eMic && canal.limpeza.eco}
          disabled={!eMic}
          onClick={() => som.mudar(canal.id, { limpeza: { ...canal.limpeza, eco: !canal.limpeza.eco } })}
        >
          <span>{t('tv.som.eco')}</span>
          <span>{!eMic ? t('tv.som.soMicrofones') : canal.limpeza.eco ? t('tv.estado.activa') : t('tv.estado.desligada')}</span>
        </button>
        <button
          type="button"
          className="tv-limpeza"
          aria-pressed={canal.limpeza.ruido}
          data-tv="ruido"
          onClick={() => som.mudar(canal.id, { limpeza: { ...canal.limpeza, ruido: !canal.limpeza.ruido } })}
        >
          <span>{t('tv.som.ruido')}</span>
          <span>{canal.limpeza.ruido ? t('tv.som.rnnoise') : t('tv.estado.desligada')}</span>
        </button>
        <button type="button" className="tv-limpeza" disabled aria-pressed={false}>
          <span>{t('tv.som.silencios')}</span>
          <span>{t('tv.som.soNoEditor')}</span>
        </button>
        <button
          type="button"
          className="tv-limpeza"
          aria-pressed={eMic && canal.limpeza.nivelamento}
          disabled={!eMic}
          onClick={() => som.mudar(canal.id, { limpeza: { ...canal.limpeza, nivelamento: !canal.limpeza.nivelamento } })}
        >
          <span>{t('tv.som.nivelamento')}</span>
          <span>{!eMic ? t('tv.som.soMicrofones') : canal.limpeza.nivelamento ? t('tv.estado.activo') : t('tv.estado.desligado')}</span>
        </button>
      </Cartao>

      <Cartao variante="nota" style={{ marginTop: 'auto' }}>
        <h3 className="tv-t3">{t('tv.som.faixasSeparadas')}</h3>
        <p className="tv-nota">{t('tv.som.faixasSeparadasNota')}</p>
      </Cartao>
    </>
  )
}

/** A curva do EQ (calculada) por cima do espectro MEDIDO do canal depois do EQ. */
function Espectro({ s, canal }: { s: PropsDoEcra['s']; canal: EstadoDoCanal }) {
  const ref = useRef<HTMLCanvasElement>(null)
  const dados = useRef<Float32Array<ArrayBuffer> | null>(null)
  const canalRef = useRef(canal)
  canalRef.current = canal
  const { t } = useTranslation()
  useTique(() => {
    const cv = ref.current
    const som = s.som
    if (!cv || !som) return
    const W = cv.clientWidth || 276
    const H = cv.clientHeight || 80
    if (cv.width !== W) cv.width = W
    if (cv.height !== H) cv.height = H
    const g = cv.getContext('2d')
    if (!g) return
    const estilo = getComputedStyle(cv)
    g.clearRect(0, 0, W, H)
    g.strokeStyle = estilo.getPropertyValue('--border') || '#2a2a2e'
    g.lineWidth = 1
    g.beginPath()
    g.moveTo(0, H / 2 + 0.5)
    g.lineTo(W, H / 2 + 0.5)
    g.stroke()
    const x = (f: number) => (Math.log10(f / 20) / 3) * W
    // Espectro medido (dBFS −100…−20 → altura).
    const n = som.binsDoEspectro(canalRef.current.id)
    if (n) {
      if (!dados.current || dados.current.length !== n) dados.current = new Float32Array(new ArrayBuffer(n * 4))
      if (som.espectro(canalRef.current.id, dados.current)) {
        const nyq = som.ctx.sampleRate / 2
        g.fillStyle = estilo.getPropertyValue('--tv-espectro') || 'rgba(255,90,96,0.18)'
        g.beginPath()
        g.moveTo(0, H)
        for (let i = 1; i < n; i++) {
          const f = (i / n) * nyq
          if (f < 20) continue
          const v = Math.min(1, Math.max(0, (dados.current[i] + 100) / 80))
          g.lineTo(x(f), H - v * H)
        }
        g.lineTo(W, H)
        g.closePath()
        g.fill()
      }
    }
    const curva = curvaDoEq(canalRef.current.eqLigado ? canalRef.current.eq : [], 96, som.ctx.sampleRate)
    g.strokeStyle = estilo.getPropertyValue('--accent') || '#ff5a60'
    g.lineWidth = 1.5
    g.beginPath()
    curva.forEach((p, i) => {
      const y = H / 2 - (p.db / EQ_GANHO_MAXIMO) * (H / 2 - 6)
      if (i === 0) g.moveTo(x(p.f), y)
      else g.lineTo(x(p.f), y)
    })
    g.stroke()
  })
  return (
    <div className="tv-eq">
      <canvas ref={ref} data-tv="espectro" aria-hidden="true" />
      <span className="tv-eq__lim" style={{ top: 6 }}>
        {t('tv.unidades.db', { v: `+${EQ_GANHO_MAXIMO}` })}
      </span>
      <span className="tv-eq__lim" style={{ bottom: 6 }}>
        {t('tv.unidades.db', { v: `−${EQ_GANHO_MAXIMO}` })}
      </span>
    </div>
  )
}
