/**
 * Palco da pré-visualização: V1 e V2 em `<video>`, A1/A2 em `<audio>`, e por
 * cima os textos, as legendas (até duas línguas) e a marca de água — o mesmo
 * que a exportação queima, desenhado em DOM.
 *
 * Duas formas, as do template: `edicao` (etiquetas em cima, tempo e «⟲ 5 s ·
 * 1,0×» em baixo) e `legendas` (orador e língua em cima, barra de posição em
 * baixo, dentro do vídeo).
 */
import type { CSSProperties, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { cx } from '../../ui/kit'
import { relogio } from '../captions/legendas'
import { camadaDeTemperatura, filtroCss, posicaoNoCanto } from './cor'
import type { Clip, Cue, Projecto } from './projecto'
import { cueEm, duracaoDoProjecto, estadoDaFaixa } from './projecto'
import type { Leitor } from './useLeitor'

const RITMOS = [0.5, 1, 1.5, 2]

function estiloDoClip(c: Clip | null | undefined, t: number): CSSProperties {
  if (!c) return { visibility: 'hidden' }
  const m = c.mascara
  let opacity = 1
  if (c.transicao && t - c.inicio < c.transicao.duracao) opacity = Math.max(0, Math.min(1, (t - c.inicio) / c.transicao.duracao))
  return {
    filter: filtroCss(c.cor),
    opacity,
    clipPath: m ? `inset(${m.y * 100}% ${(1 - m.x - m.w) * 100}% ${(1 - m.y - m.h) * 100}% ${m.x * 100}%)` : undefined,
  }
}

function Temperatura({ c, t }: { c: Clip | null | undefined; t: number }) {
  const temp = c ? camadaDeTemperatura(c.cor) : null
  if (!c || !temp) return null
  const s = estiloDoClip(c, t)
  return <span className="ed-temp" aria-hidden="true" style={{ background: `rgb(${temp.rgb} / ${temp.alfa})`, clipPath: s.clipPath, opacity: s.opacity }} />
}

function Linha({ cue, t, karaoke }: { cue: Cue; t: number; karaoke: boolean }) {
  if (karaoke && cue.palavras?.length) {
    return (
      <>
        {cue.palavras.map((w, i) => (
          <span key={i} className={cx('ed-cap__w', t >= w.inicio && 'ed-cap__w--dito')}>
            {w.texto}{' '}
          </span>
        ))}
      </>
    )
  }
  return <>{cue.texto}</>
}

export default function Preview({
  projecto: p,
  leitor,
  lingua,
  marcaDeAgua,
  forma,
  topo,
  onLegendas,
}: {
  projecto: Projecto
  leitor: Leitor
  /** Língua das legendas mostradas, ou `null` para esconder. */
  lingua: string | null
  marcaDeAgua: string
  forma: 'edicao' | 'legendas'
  /** Etiquetas do canto superior esquerdo na forma `legendas`. */
  topo?: ReactNode
  onLegendas?: () => void
}) {
  const { t, i18n } = useTranslation()
  const T = leitor.tempo
  const dur = duracaoDoProjecto(p)
  const { V1, V2 } = leitor.activos
  const v1 = estadoDaFaixa(p, 'V1').visivel ? V1 : null
  const v2 = estadoDaFaixa(p, 'V2').visivel ? V2 : null
  const ccVisivel = estadoDaFaixa(p, 'CC').visivel
  const cue = lingua && ccVisivel ? cueEm(p, T, lingua) : null
  const segundaLingua = p.estilo.segundaLingua && p.estilo.segundaLingua !== lingua ? p.estilo.segundaLingua : null
  const cue2 = segundaLingua && ccVisivel ? cueEm(p, T, segundaLingua) : null
  const karaoke = p.estilo.modo === 'karaoke'
  const pos = posicaoNoCanto(p.marca.canto, 0, 0)

  return (
    <div className={cx('ed-stage', `ed-stage--${forma}`)}>
      <div className="ed-stage__frame" style={{ aspectRatio: `${p.largura} / ${p.altura}` }}>
        <video ref={leitor.ligar.V1} className="ed-stage__v" muted playsInline data-studio="preview" style={estiloDoClip(v1, T)} />
        <Temperatura c={v1} t={T} />
        <video ref={leitor.ligar.V2} className="ed-stage__v" muted playsInline style={estiloDoClip(v2, T)} />
        <Temperatura c={v2} t={T} />
        <audio ref={leitor.ligar.A1} hidden />
        <audio ref={leitor.ligar.A2} hidden />

        {p.textos
          .filter((x) => T >= x.inicio && T < x.inicio + x.duracao)
          .map((x) => (
            <span key={x.id} className="ed-stage__text" style={{ left: `${x.x * 100}%`, top: `${x.y * 100}%`, fontSize: `${x.tamanho * 100}cqh` }}>
              {x.texto}
            </span>
          ))}

        {(cue || cue2) && (
          <div className={cx('ed-cap', `ed-cap--${p.estilo.modo}`)} style={{ fontSize: `${(p.estilo.tamanho / 1080) * 100}cqh` }}>
            {cue && (
              <span className="ed-cap__line">
                <Linha cue={cue} t={T} karaoke={karaoke} />
              </span>
            )}
            {cue2 && <span className="ed-cap__line ed-cap__line--2">{cue2.texto}</span>}
          </div>
        )}

        {p.marca.marcaDeAgua && marcaDeAgua && (
          <span
            className="ed-stage__mark"
            style={{ opacity: p.marca.opacidade, [pos.x > 0.5 ? 'right' : 'left']: '3%', [pos.y > 0.5 ? 'bottom' : 'top']: '3%' }}
          >
            {marcaDeAgua}
          </span>
        )}
      </div>

      {!V1 && !V2 && <span className="ed-stage__empty">{p.clips.length ? t('editor.preview.semImagem') : t('editor.preview.vazio')}</span>}

      {forma === 'edicao' ? (
        <>
          <div className="ed-badges">
            <span className="ed-badge dx-num">{t('editor.preview.rotulo', { p: p.altura })}</span>
            {p.legendas && (
              <button type="button" className="ed-badge ed-badge--ok dx-num" onClick={onLegendas}>
                {t('editor.preview.legendas', { lingua: lingua ?? p.legendas.lingua })}
              </button>
            )}
          </div>
          <span className="ed-stage__tc dx-num" data-studio="tempo-preview">
            {relogio(T).padStart(5, '0')} / {relogio(dur).padStart(5, '0')}
          </span>
          <div className="ed-stage__ctl">
            <button type="button" className="ed-badge dx-num" onClick={() => leitor.buscar(T - 5)} aria-label={t('editor.preview.recuar')}>
              {t('editor.preview.cincoS')}
            </button>
            <button
              type="button"
              className="ed-badge dx-num"
              aria-label={t('editor.preview.ritmo')}
              onClick={() => leitor.setRitmo(RITMOS[(RITMOS.indexOf(leitor.ritmo) + 1) % RITMOS.length])}
            >
              {leitor.ritmo.toLocaleString(i18n.language, { minimumFractionDigits: 1 })}×
            </button>
          </div>
        </>
      ) : (
        <>
          {topo && <div className="ed-badges">{topo}</div>}
          <div className="ed-stage__scrub">
            <span className="dx-num">{relogio(T).padStart(5, '0')}</span>
            <input
              type="range"
              className="ed-range"
              min={0}
              max={Math.max(0.1, dur)}
              step={0.1}
              value={T}
              style={{ ['--pct' as string]: `${dur ? (T / dur) * 100 : 0}%` }}
              aria-label={t('editor.legendas.posicao')}
              onChange={(e) => leitor.buscar(Number(e.target.value))}
            />
            <span className="dx-num">{relogio(dur).padStart(5, '0')}</span>
          </div>
        </>
      )}
    </div>
  )
}
