/**
 * DelonixSources — fontes e dispositivos.
 *
 * Funcional no browser: descobrir os dispositivos de vídeo (`enumerateDevices`),
 * ligar e desligar câmaras, ver cada fonte com a resolução, os fps e o atraso
 * MEDIDOS, arrumar o barramento, e controlar a câmara local pelo que o
 * dispositivo expõe.
 *
 * Do servidor (contrato do estúdio de TV, ainda por entregar): o código de
 * emparelhamento da app Delonix Câmara, o estado do telefone e a gravação ISO
 * por fonte. Aparecem com o estado honesto «a aguardar servidor».
 */
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { tallyDe } from '../../../studio/tv/mesa'
import { Icon } from '../../../ui/icons'
import { cx, IconButton } from '../../../ui/kit'
import { type PropsDoEcra, SelosDoAr } from './comum'
import { bloquearAeAf, ControlosDaCamara, podeBloquear, podeFocarUmaVez } from './ControlosDaCamara'
import { BotaoTv, Cabeca, Cartao, Espaco, Miniatura, TopoTv, useTique } from './pecas'

export default function Fontes({ s, c }: PropsDoEcra) {
  const { t } = useTranslation()
  const lista = s.registo.lista()
  const [escolhida, setEscolhida] = useState('')
  const [aProcurar, setAProcurar] = useState(false)
  const fonte = lista.find((f) => f.id === escolhida) ?? lista[0] ?? null
  const noAr = new Set([...(s.mesa.programa?.fontes ?? []), ...(s.mesa.emCurso?.para.fontes ?? [])])
  const ligadas = new Set(lista.filter((f) => f.tipo === 'camara').map((f) => f.deviceId))
  const participantes = lista.filter((f) => f.tipo === 'participante').length

  const procurar = async () => {
    setAProcurar(true)
    try {
      // Sem permissão, o browser esconde os nomes: pede-se uma vez e larga-se logo.
      if (s.camaras.some((x) => !x.deviceId || /^\S+ \d+$/.test(x.nome)) || !s.camaras.length) {
        const st = await navigator.mediaDevices.getUserMedia({ video: true }).catch(() => null)
        st?.getTracks().forEach((x) => x.stop())
      }
      await s.procurarDispositivos()
    } finally {
      setAProcurar(false)
    }
  }

  return (
    <div className="dx-stage tv" data-tv-ecra="fontes">
      <TopoTv titulo={t('tv.fontes.titulo')} onVoltar={() => c.onNavegar(null)}>
        <span className="tv-chip" data-tv="contagem-fontes">
          {t('tv.fontes.contagem', { count: lista.length, noAr: noAr.size })}
        </span>
        <SelosDoAr c={c} destinosNoSelo={false} />
        <Espaco />
        <BotaoTv disabled={aProcurar} data-tv="procurar" onClick={() => void procurar()}>
          {t('tv.fontes.procurar')}
        </BotaoTv>
        <BotaoTv variante="forte" onClick={() => c.onNavegar('mesa-de-corte')}>
          {t('tv.fontes.abrirMesa')}
        </BotaoTv>
      </TopoTv>

      <div className="tv-corpo tv-fontes">
        <aside className="tv-col tv-col--esq" style={{ gap: 12 }} aria-label={t('tv.fontes.ligacoes')}>
          <p className="tv-eyebrow">{t('tv.fontes.ligacoes')}</p>
          <div className="tv-rolar" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <section className={cx('tv-ligacao', ligadas.size > 0 && 'is-on')} data-tv="ligacao-usb">
              <div className="tv-ligacao__titulo">
                <Icon name="video" />
                <span>{t('tv.fontes.usb')}</span>
                <span className="tv-dir tv-mono-85">{t('tv.fontes.nDispositivos', { count: s.camaras.length })}</span>
              </div>
              <p className="tv-nota">{t('tv.fontes.usbNota')}</p>
              <p className="tv-nota" data-tv="usb-telemovel">{t('tv.fontes.usbTelemovel')}</p>
              {s.camaras.length > 0 && (
                <ul className="tv-dispositivos">
                  {s.camaras.map((d) => {
                    const ligada = ligadas.has(d.deviceId)
                    return (
                      <li key={d.deviceId || d.nome}>
                        <span title={d.nome}>{d.nome}</span>
                        <BotaoTv
                          mini
                          variante={ligada ? undefined : 'accent'}
                          data-tv="ligar-camara"
                          onClick={() => (ligada ? s.registo.desligar(`camara:${d.deviceId}`) : void s.ligarCamara(d))}
                        >
                          {ligada ? t('tv.fontes.desligar') : t('tv.fontes.ligar')}
                        </BotaoTv>
                      </li>
                    )
                  })}
                </ul>
              )}
              {s.erroFonte && <p className="tv-erro">{s.erroFonte}</p>}
            </section>

            <section className="tv-ligacao" data-tv="ligacao-app">
              <div className="tv-ligacao__titulo">
                <Icon name="phone" />
                <span>{t('tv.fontes.app')}</span>
                <span className="tv-dir tv-mono-85">{t('tv.aguardarServidorCurto')}</span>
              </div>
              <p className="tv-nota">{t('tv.fontes.appNota')}</p>
            </section>

            <section className={cx('tv-ligacao', participantes > 0 && 'is-on')}>
              <div className="tv-ligacao__titulo">
                <Icon name="people" />
                <span>{t('tv.fontes.sala')}</span>
                <span className="tv-dir tv-mono-85">{t('tv.fontes.nParticipantes', { count: participantes })}</span>
              </div>
              <p className="tv-nota">{participantes ? t('tv.fontes.salaNota') : t('tv.fontes.salaSem')}</p>
            </section>

            <section className="tv-ligacao">
              <div className="tv-ligacao__titulo">
                <Icon name="screen" />
                <span>{t('tv.fontes.capturadora')}</span>
              </div>
              <p className="tv-nota">{t('tv.fontes.capturadoraNota')}</p>
            </section>
          </div>

          <Cartao variante="tracejado" style={{ marginTop: 'auto', padding: 9 }} data-tv="telefone-sem-cabo">
            <h2 className="tv-t3">{t('tv.fontes.semCabo')}</h2>
            <p className="tv-nota">{t('tv.fontes.semCaboNota')}</p>
            <span className="tv-selo tv-selo--espera" style={{ justifyContent: 'center' }}>
              {t('tv.aguardarServidor')}
            </span>
          </Cartao>
        </aside>

        <section className="tv-centro" aria-label={t('tv.fontes.lista')}>
          {lista.length === 0 ? (
            <div className="tv-vazio" style={{ flex: 'none', minHeight: 200 }}>
              <strong>{t('tv.fontes.vazio')}</strong>
              <span>{t('tv.fontes.vazioNota')}</span>
            </div>
          ) : (
            <div className="tv-grelha-fontes tv-rolar" style={{ flex: '0 1 auto' }}>
              {lista.map((f, i) => {
                const tally = tallyDe(s.mesa, f.id)
                return (
                  <CartaoDaFonte
                    key={f.id}
                    s={s}
                    id={f.id}
                    numero={i + 1}
                    tally={tally}
                    escolhida={fonte?.id === f.id}
                    onEscolher={() => setEscolhida(f.id)}
                    podeSubir={i > 0}
                    podeDescer={i < lista.length - 1}
                  />
                )
              })}
            </div>
          )}

          <Cartao como="section" variante="painel" style={{ flex: 1, minHeight: 120, gap: 9 }} aria-labelledby="tv-iso-h" data-tv="iso">
            <Cabeca>
              <h2 id="tv-iso-h" className="tv-t1" style={{ fontSize: 12 }}>
                {t('tv.fontes.iso')}
              </h2>
              <span className="tv-mono-9">{t('tv.fontes.isoDica')}</span>
              <span className="tv-dir tv-selo tv-selo--espera">{t('tv.aguardarServidor')}</span>
            </Cabeca>
            <p className="tv-nota">{t('tv.fontes.isoNota')}</p>
            <Cabeca className="tv-mono-9" style={{ marginTop: 'auto' }}>
              <span>{t('tv.fontes.isoDestino')}</span>
              <span className="tv-dir">{t('tv.fontes.isoLocal')}</span>
            </Cabeca>
          </Cartao>
        </section>

        <aside className="tv-col tv-col--dir" aria-label={t('tv.fontes.detalhe')}>
          {fonte ? <Detalhe s={s} id={fonte.id} numero={lista.indexOf(fonte) + 1} /> : <div className="tv-vazio">{t('tv.fontes.escolhe')}</div>}
          <Cartao variante="accent" style={{ marginTop: 'auto', gap: 5 }}>
            <h3 className="tv-t3">{t('tv.fontes.dicaTitulo')}</h3>
            <p className="tv-nota">{t('tv.fontes.dica')}</p>
          </Cartao>
        </aside>
      </div>
    </div>
  )
}

function CartaoDaFonte({
  s,
  id,
  numero,
  tally,
  escolhida,
  onEscolher,
  podeSubir,
  podeDescer,
}: {
  s: PropsDoEcra['s']
  id: string
  numero: number
  tally: 'programa' | 'previa' | 'livre'
  escolhida: boolean
  onEscolher: () => void
  podeSubir: boolean
  podeDescer: boolean
}) {
  const { t } = useTranslation()
  const f = s.registo.obter(id)!
  const medidas = useRef<HTMLSpanElement>(null)
  const atraso = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (agora - ultimo.current < 500) return
    ultimo.current = agora
    const m = s.registo.medidas(id)
    if (medidas.current) {
      medidas.current.textContent = m && m.altura ? t('tv.fontes.formato', { h: m.altura, fps: m.fps ?? '—' }) : t('tv.fontes.semImagem')
    }
    if (atraso.current) {
      atraso.current.hidden = !m || m.latenciaMs === null
      if (m && m.latenciaMs !== null) {
        atraso.current.textContent = t('tv.unidades.ms', { v: m.latenciaMs })
        atraso.current.classList.toggle('is-lento', m.latenciaMs > 100)
      }
    }
  })
  const tipoTag = t(`tv.fontes.tipos.${f.tipo}`)
  return (
    <div
      className={cx('tv-fonte', tally === 'programa' && 'is-pgm', tally === 'previa' && 'is-pre', escolhida && 'is-sel')}
      data-tv="fonte"
      data-fonte={id}
      role="button"
      tabIndex={0}
      aria-pressed={escolhida}
      onClick={onEscolher}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          e.stopPropagation()
          onEscolher()
        }
      }}
    >
      <span className="tv-fonte__img">
        <Miniatura imagem={() => s.registo.imagem(id)} ajuste={s.registo.ajuste(id)} largura={384} altura={216} />
        <span className="tv-mini__num">{numero <= 6 ? numero : ''}</span>
        <span className="tv-mini__tally">{t(`tv.tally.${tally}`)}</span>
        <span className="tv-fonte__medidas">
          <span ref={medidas} data-tv="formato-medido" />
          <span ref={atraso} data-tv="atraso-medido" hidden />
        </span>
      </span>
      <span className="tv-fonte__pe">
        <Cabeca como="span">
          <span className="tv-fonte__nome">{f.nome}</span>
          <span className="tv-dir tv-tag">{numero <= 6 ? t(`tv.fontes.barramento.${tally}`) : t('tv.fontes.foraDoBarramento')}</span>
        </Cabeca>
        {f.dispositivo && f.dispositivo !== f.nome && <span className="tv-fonte__disp">{f.dispositivo}</span>}
        <span className="tv-tags">
          <span className="tv-tag">{tipoTag}</span>
          <span className="tv-ordem">
            <IconButton icon="chevronLeft" bare label={t('tv.fontes.subir', { nome: f.nome })} disabled={!podeSubir} onClick={(e) => (e.stopPropagation(), s.registo.mover(id, -1))} />
            <IconButton icon="chevronRight" bare label={t('tv.fontes.descer', { nome: f.nome })} disabled={!podeDescer} onClick={(e) => (e.stopPropagation(), s.registo.mover(id, 1))} />
            {f.tipo === 'camara' && <IconButton icon="x" bare label={t('tv.fontes.desligarAria', { nome: f.nome })} onClick={(e) => (e.stopPropagation(), s.registo.desligar(id))} />}
          </span>
        </span>
      </span>
    </div>
  )
}

function Detalhe({ s, id, numero }: { s: PropsDoEcra['s']; id: string; numero: number }) {
  const { t } = useTranslation()
  const f = s.registo.obter(id)!
  const track = f.stream?.getVideoTracks()[0] ?? null
  const local = f.tipo === 'camara'
  const [bloqueado, setBloqueado] = useState(false)
  const [focado, setFocado] = useState('')
  const valores = useRef<HTMLDivElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (agora - ultimo.current < 500 || !valores.current) return
    ultimo.current = agora
    const m = s.registo.medidas(id)
    const spans = valores.current.querySelectorAll<HTMLSpanElement>('[data-v]')
    for (const el of spans) {
      const k = el.dataset.v
      if (k === 'res') el.textContent = m?.largura ? `${m.largura}×${m.altura}` : '—'
      if (k === 'fps') el.textContent = m?.fps != null ? String(m.fps) : '—'
      if (k === 'atraso') el.textContent = m?.latenciaMs != null ? t('tv.unidades.ms', { v: m.latenciaMs }) : t('tv.fontes.semMedida')
    }
  })
  return (
    <>
      <Cabeca>
        <h2 className="tv-t1" style={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
          {numero <= 6 ? t('tv.fontes.detalheTitulo', { n: numero, nome: f.nome }) : f.nome}
        </h2>
        <span className="tv-dir tv-tag tv-tag--forte">{t(`tv.fontes.tipos.${f.tipo}`)}</span>
      </Cabeca>

      {local && (
        <Cartao como="section" style={{ gap: 9 }} data-tv="controlo-camara">
          <span className="tv-mono-85">{t('tv.fontes.controlo')}</span>
          <ControlosDaCamara track={track} />
          <Cabeca style={{ gap: 5 }}>
            <BotaoTv
              variante="forte"
              style={{ flex: 1, padding: 5, fontSize: 10 }}
              disabled={!podeFocarUmaVez(track)}
              title={podeFocarUmaVez(track) ? undefined : t('tv.camara.semSuporte')}
              onClick={() => {
                if (!track) return
                void track
                  .applyConstraints({ advanced: [{ focusMode: 'single-shot' } as MediaTrackConstraintSet] })
                  .then(() => setFocado(t('tv.fontes.focado')))
                  .catch(() => setFocado(t('tv.camara.recusado')))
              }}
            >
              {t('tv.fontes.focar')}
            </BotaoTv>
            <BotaoTv
              style={{ flex: 1, padding: 5, fontSize: 10 }}
              aria-pressed={bloqueado}
              disabled={!podeBloquear(track)}
              title={podeBloquear(track) ? undefined : t('tv.camara.semSuporte')}
              onClick={() => track && void bloquearAeAf(track, !bloqueado).then((ok) => ok && setBloqueado(!bloqueado))}
            >
              {bloqueado ? t('tv.fontes.desbloquearAe') : t('tv.fontes.bloquearAe')}
            </BotaoTv>
          </Cabeca>
          {focado && <span className="tv-mono-85">{focado}</span>}
        </Cartao>
      )}

      <Cartao como="section" style={{ gap: 7 }}>
        <h3 className="tv-t2">{t('tv.fontes.estado')}</h3>
        <div ref={valores} style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
          <div className="tv-kv">
            <span>{t('tv.fontes.resolucao')}</span>
            <span data-v="res" />
          </div>
          <div className="tv-kv">
            <span>{t('tv.fontes.fps')}</span>
            <span data-v="fps" />
          </div>
          <div className="tv-kv">
            <span>{t('tv.fontes.atraso')}</span>
            <span data-v="atraso" className="tv-ok" />
          </div>
          {f.dispositivo && (
            <div className="tv-kv">
              <span>{t('tv.fontes.dispositivo')}</span>
              <span style={{ maxWidth: 150, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={f.dispositivo}>
                {f.dispositivo}
              </span>
            </div>
          )}
          {!local && f.tipo === 'participante' && (
            <div className="tv-kv">
              <span>{t('tv.fontes.estadoTelefone')}</span>
              <span className="tv-muted">{t('tv.aguardarServidorCurto')}</span>
            </div>
          )}
        </div>
      </Cartao>

      <Cartao como="section" style={{ gap: 6 }}>
        <h3 className="tv-t2">{t('tv.fontes.sincronismo')}</h3>
        <p className="tv-nota">{t('tv.fontes.sincronismoNota')}</p>
        <div className="tv-kv">
          <span>{t('tv.fontes.diferenca')}</span>
          <SincValor s={s} />
        </div>
      </Cartao>
    </>
  )
}

function SincValor({ s }: { s: PropsDoEcra['s'] }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLSpanElement>(null)
  const ultimo = useRef(0)
  useTique((agora) => {
    if (agora - ultimo.current < 500 || !ref.current) return
    ultimo.current = agora
    const todas = s.registo
      .lista()
      .map((x) => s.registo.medidas(x.id)?.latenciaMs)
      .filter((x): x is number => x !== null && x !== undefined)
    ref.current.textContent = todas.length >= 2 ? t('tv.fontes.sincValor', { ms: Math.round((Math.max(...todas) - Math.min(...todas)) / 2) }) : t('tv.fontes.semMedida')
  })
  return <span ref={ref} className="tv-ok" data-tv="sincronismo" />
}
