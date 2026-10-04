/**
 * DelonixLighting — iluminação e imagem.
 *
 * O que é do browser e funciona já: a correcção por software de cada fonte
 * (exposição, temperatura, matiz, contraste, saturação) com antes/depois e a
 * luminância MEDIDA nos dois monitores, e o equilíbrio de brancos entre
 * câmaras pelo cinzento médio.
 *
 * O que é do agente local de iluminação: os aparelhos e as cenas de luz. O
 * agente não existe — o ecrã diz «sem agente» e não nomeia protocolos que
 * nenhum código fala (o portão `check-capability-claims.sh` recusa-os).
 */
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CORRECCAO_NEUTRA, type CorreccaoDeImagem, equilibrioCinzento, LIMITES, luminanciaMedia, mediaRgb } from '../../../studio/tv/correccao'
import { desenharImagem } from '../../../studio/tv/desenhoDaMesa'
import { formatarDb } from '../../../studio/tv/som'
import { type PropsDoEcra, SelosDoAr } from './comum'
import { Deslizador, Espaco, Interruptor, TopoTv, useTique } from './pecas'

const W = 480
const H = 270

export default function Iluminacao({ s, c }: PropsDoEcra) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const fontes = s.registo.lista()
  const camaras = fontes.filter((f) => f.tipo === 'camara' || f.tipo === 'participante')
  const [escolhida, setEscolhida] = useState('')
  const fonte = fontes.find((f) => f.id === escolhida) ?? camaras[0] ?? fontes[0] ?? null
  const [, forcar] = useState(0)
  const corr: CorreccaoDeImagem = fonte ? s.registo.correccao(fonte.id) : CORRECCAO_NEUTRA
  const completa = fonte ? s.registo.correccaoCompleta(fonte.id) : true
  const mudar = (patch: Partial<CorreccaoDeImagem>) => {
    if (!fonte) return
    s.registo.definirCorreccao(fonte.id, { ...corr, ...patch })
    forcar((n) => n + 1)
  }
  const equilibrioLigado = camaras.some((f) => s.registo.correccao(f.id).equilibrio.some((x) => x !== 1))
  const [aEquilibrar, setAEquilibrar] = useState(false)

  /** Mede o cinzento médio de cada câmara (sem correcção) e iguala os brancos. */
  const equilibrar = (ligar: boolean) => {
    setAEquilibrar(true)
    const cv = document.createElement('canvas')
    cv.width = 160
    cv.height = 90
    const g = cv.getContext('2d', { willReadFrequently: true })
    for (const f of camaras) {
      const atual = s.registo.correccao(f.id)
      let equilibrio: [number, number, number] = [1, 1, 1]
      if (ligar && g) {
        desenharImagem(g, s.registo.imagemCrua(f.id), { x: 0, y: 0, w: 160, h: 90 }, 'cover')
        equilibrio = equilibrioCinzento(mediaRgb(g.getImageData(0, 0, 160, 90).data, 2))
      }
      s.registo.definirCorreccao(f.id, { ...atual, equilibrio })
    }
    setAEquilibrar(false)
    forcar((n) => n + 1)
  }

  return (
    <div className="dx-stage tv" data-tv-ecra="iluminacao">
      <TopoTv titulo={t('tv.luz.titulo')} onVoltar={() => c.onNavegar(null)}>
        <span className="tv-chip">{t('tv.luz.semAparelhos')}</span>
        <SelosDoAr c={c} destinosNoSelo={false} />
        <Espaco />
        <span className="tv-selo tv-selo--espera" data-tv="agente-luz" title={t('tv.aguardarServidor')}>
          {t('tv.luz.semAgente')}
        </span>
        <button type="button" className="tv-botao tv-botao--forte" onClick={() => c.onNavegar('mesa-de-corte')}>
          {t('tv.nav.mesaDeCorte')}
        </button>
      </TopoTv>

      <div className="tv-corpo tv-luz">
        <aside className="tv-col tv-col--esq" aria-label={t('tv.luz.cenas')} style={{ gap: 10 }}>
          <p className="tv-eyebrow">{t('tv.luz.cenas')}</p>
          <div className="tv-cartao tv-cartao--tracejado" data-tv="cenas-luz">
            <h2 className="tv-t3">{t('tv.luz.cenasSemAgente')}</h2>
            <p className="tv-nota">{t('tv.luz.cenasSemAgenteNota')}</p>
          </div>
          <div className="tv-cartao tv-cartao--nota" style={{ marginTop: 'auto' }}>
            <p className="tv-eyebrow">{t('tv.luz.fontes')}</p>
            {fontes.length === 0 ? (
              <p className="tv-nota">{t('tv.luz.semFontes')}</p>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
                {fontes.map((f) => {
                  const k = s.registo.correccao(f.id)
                  return (
                    <button
                      key={f.id}
                      type="button"
                      className="tv-limpeza"
                      aria-pressed={fonte?.id === f.id}
                      onClick={() => setEscolhida(f.id)}
                      data-tv="fonte-luz"
                    >
                      <span>{f.nome}</span>
                      <span>{t('tv.luz.resumo', { ev: formatarDb(k.exposicao, lang), k: Math.round(k.temperatura).toLocaleString(lang) })}</span>
                    </button>
                  )
                })}
              </div>
            )}
          </div>
        </aside>

        <section className="tv-centro" aria-label={t('tv.luz.imagem')}>
          <div className="tv-antes-depois">
            <Monitor s={s} id={fonte?.id ?? null} corrigido={false} />
            <Monitor s={s} id={fonte?.id ?? null} corrigido />
          </div>
          <section className="tv-cartao tv-cartao--painel" style={{ flex: 1, minHeight: 0 }} aria-labelledby="tv-aparelhos-h">
            <div className="tv-cabeca">
              <h2 id="tv-aparelhos-h" className="tv-t1" style={{ fontSize: 12 }}>
                {t('tv.luz.aparelhos')}
              </h2>
            </div>
            <div className="tv-vazio" data-tv="aparelhos-vazio">
              <strong>{t('tv.luz.semAgente')}</strong>
              <span>{t('tv.luz.semAgenteNota')}</span>
            </div>
          </section>
        </section>

        <aside className="tv-col tv-col--dir" aria-label={t('tv.luz.correccao')}>
          <h2 className="tv-t1">{t('tv.luz.correccao')}</h2>
          <section className="tv-cartao" style={{ gap: 9 }} data-tv="correccao">
            <div className="tv-cabeca">
              <span className="tv-mono-85" style={{ textTransform: 'uppercase' }}>
                {fonte ? fonte.nome : t('tv.luz.semFonte')}
              </span>
              {fonte && (
                <button type="button" className="tv-dir tv-botao tv-botao--mini" onClick={() => mudar({ ...CORRECCAO_NEUTRA, equilibrio: corr.equilibrio })}>
                  {t('tv.luz.repor')}
                </button>
              )}
            </div>
            <Deslizador
              rotulo={t('tv.luz.ganho')}
              valor={corr.exposicao}
              min={LIMITES.exposicao[0]}
              max={LIMITES.exposicao[1]}
              passo={0.1}
              disabled={!fonte}
              texto={t('tv.unidades.ev', { v: formatarDb(corr.exposicao, lang) })}
              onChange={(v) => mudar({ exposicao: v })}
              larguraRotulo={82}
              larguraValor={50}
              data-tv="exposicao"
            />
            <Deslizador
              rotulo={t('tv.luz.temperatura')}
              valor={corr.temperatura}
              min={LIMITES.temperatura[0]}
              max={LIMITES.temperatura[1]}
              passo={100}
              disabled={!fonte || !completa}
              title={completa ? undefined : t('tv.luz.semWebgl')}
              texto={t('tv.unidades.kelvin', { v: corr.temperatura.toLocaleString(lang) })}
              onChange={(v) => mudar({ temperatura: v })}
              larguraRotulo={82}
              larguraValor={50}
            />
            <Deslizador
              rotulo={t('tv.luz.matiz')}
              valor={corr.matiz}
              min={LIMITES.matiz[0]}
              max={LIMITES.matiz[1]}
              disabled={!fonte || !completa}
              title={completa ? undefined : t('tv.luz.semWebgl')}
              texto={String(corr.matiz)}
              onChange={(v) => mudar({ matiz: v })}
              larguraRotulo={82}
              larguraValor={50}
            />
            <Deslizador
              rotulo={t('tv.luz.contraste')}
              valor={corr.contraste}
              min={LIMITES.contraste[0]}
              max={LIMITES.contraste[1]}
              passo={0.05}
              disabled={!fonte}
              texto={corr.contraste.toLocaleString(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
              onChange={(v) => mudar({ contraste: v })}
              larguraRotulo={82}
              larguraValor={50}
            />
            <Deslizador
              rotulo={t('tv.luz.saturacao')}
              valor={corr.saturacao}
              min={LIMITES.saturacao[0]}
              max={LIMITES.saturacao[1]}
              passo={0.05}
              disabled={!fonte}
              texto={corr.saturacao.toLocaleString(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
              onChange={(v) => mudar({ saturacao: v })}
              larguraRotulo={82}
              larguraValor={50}
            />
            {!completa && <p className="tv-nota tv-aviso">{t('tv.luz.semWebgl')}</p>}
          </section>

          <section className="tv-cartao" style={{ gap: 7 }}>
            <h3 className="tv-t2">{t('tv.luz.tratamento')}</h3>
            <div className="tv-cabeca" style={{ gap: 9 }}>
              <Interruptor
                pequeno
                ligado={equilibrioLigado}
                disabled={camaras.length < 1 || aEquilibrar}
                rotulo={t('tv.luz.equilibrio')}
                onChange={equilibrar}
              />
              <span style={{ fontSize: 10 }}>{t('tv.luz.equilibrio')}</span>
              <span className="tv-dir tv-mono-85">{t('tv.luz.nFontes', { count: camaras.length })}</span>
            </div>
            <p className="tv-nota">{t('tv.luz.equilibrioNota')}</p>
          </section>

          <div className="tv-cartao tv-cartao--nota" style={{ marginTop: 'auto' }}>
            <h3 className="tv-t3">{t('tv.luz.dicaTitulo')}</h3>
            <p className="tv-nota">{t('tv.luz.dica')}</p>
          </div>
        </aside>
      </div>
    </div>
  )
}

/** Antes (imagem crua) ou depois (corrigida), com a luminância média MEDIDA no próprio monitor. */
function Monitor({ s, id, corrigido }: { s: PropsDoEcra['s']; id: string | null; corrigido: boolean }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLCanvasElement>(null)
  const medida = useRef<HTMLSpanElement>(null)
  const ultima = useRef(0)
  const idRef = useRef(id)
  idRef.current = id
  useTique((agora) => {
    const cv = ref.current
    const g = cv?.getContext('2d', { willReadFrequently: true })
    if (!cv || !g) return
    const f = idRef.current
    desenharImagem(g, f ? (corrigido ? s.registo.imagem(f) : s.registo.imagemCrua(f)) : null, { x: 0, y: 0, w: W, h: H }, f ? s.registo.ajuste(f) : 'cover')
    if (agora - ultima.current > 400 && medida.current) {
      ultima.current = agora
      const l = luminanciaMedia(g.getImageData(0, 0, W, H).data, 8)
      medida.current.textContent = t('tv.luz.luminancia', { v: Math.round(l) })
      medida.current.dataset.luminancia = l.toFixed(1)
    }
  })
  return (
    <div className={`tv-monitor tv-monitor--fino${corrigido ? ' tv-monitor--depois' : ''}`}>
      <div className="tv-monitor__cabeca">
        <span className="tv-monitor__estado">{corrigido ? t('tv.luz.depois') : t('tv.luz.antes')}</span>
      </div>
      <div className="tv-monitor__corpo">
        <canvas ref={ref} width={W} height={H} className="tv-monitor__imagem" data-tv={corrigido ? 'depois' : 'antes'} />
        {!id && <span className="tv-monitor__vazio">{t('tv.corte.semSinal')}</span>}
        <span ref={medida} className="tv-monitor__medida" data-tv={corrigido ? 'luminancia-depois' : 'luminancia-antes'} />
      </div>
    </div>
  )
}
