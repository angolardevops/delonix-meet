/**
 * Pós-gravação: pré-visualizar, cortar as pontas com dois cursores, remover as
 * pausas mortas, descarregar faixas e guardar.
 *
 * Só o que o `editor.ts` e o `analise.ts` fazem — um corte (um troço) e a
 * remoção de pausas (vários troços). Não há correcção de cor, mistura, legendas
 * nem pistas múltiplas: não há código por trás, e por isso não aparecem.
 */
import { RefObject, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert, Button, Empty, Field, TextInput } from '../ui/kit'
import { AnaliseDeAudio, resumo } from './analise'
import type { ResultadoDaGravacao } from './compositor'
import { mmss } from './Cronometro'

export interface Gravado {
  faixas: ResultadoDaGravacao
  url: string
  duracao: number
}

export default function EditPanel({
  resultado,
  previewRef,
  onDuracao,
  podeCortar,
  de,
  ate,
  onDe,
  onAte,
  aCortar,
  onCortar,
  analise,
  aAnalisar,
  onProcurarPausas,
  onRemoverPausas,
  onCancelarPausas,
  onDescarregar,
  titulo,
  onTitulo,
  aGuardar,
  onGuardar,
  guardado,
}: {
  resultado: Gravado | null
  previewRef: RefObject<HTMLVideoElement | null>
  onDuracao: (d: number) => void
  podeCortar: boolean
  de: number
  ate: number
  onDe: (v: number) => void
  onAte: (v: number) => void
  /** 0 = parado; senão a fracção feita. */
  aCortar: number
  onCortar: () => void
  analise: AnaliseDeAudio | null
  aAnalisar: boolean
  onProcurarPausas: () => void
  onRemoverPausas: () => void
  onCancelarPausas: () => void
  onDescarregar: (qual: 'completo' | 'video' | 'audio') => void
  titulo: string
  onTitulo: (v: string) => void
  aGuardar: boolean
  onGuardar: () => void
  guardado: string
}) {
  const { t } = useTranslation()
  const [agora, setAgora] = useState(0)

  useEffect(() => {
    const v = previewRef.current
    if (!v) return
    const tick = () => setAgora(v.currentTime)
    v.addEventListener('timeupdate', tick)
    return () => v.removeEventListener('timeupdate', tick)
  }, [previewRef, resultado?.url])

  if (!resultado) {
    return (
      <div className="st-edit st-edit--empty">
        <Empty icon="film" title={t('studio.edicao.take')}>
          {t('studio.edicao.vazio')}
        </Empty>
      </div>
    )
  }

  const dur = resultado.duracao
  const temDuracao = dur > 0 && Number.isFinite(dur)
  const max = Math.floor(dur)
  const pct = (s: number) => `${temDuracao ? Math.min(100, (s / dur) * 100) : 0}%`
  const corteInteiro = de === 0 && Math.round(ate) >= max
  const r = analise ? resumo(analise) : null
  const aTrabalhar = aCortar > 0
  const rotuloProgresso = t('studio.edicao.aCortar', { pct: Math.round(aCortar * 100) })

  function lerDuracao(v: HTMLVideoElement) {
    const d = v.duration
    // Um WebM de MediaRecorder chega muitas vezes com duração `Infinity` até
    // se procurar até ao fim: sem isto os cursores de corte nasciam sem escala.
    if (Number.isFinite(d) && d > 0) onDuracao(d)
    else v.currentTime = 1e6
  }

  function buscar(s: number) {
    if (previewRef.current) previewRef.current.currentTime = s
  }

  return (
    <div className="st-edit">
      <aside className="st-col st-col--left" aria-label={t('studio.edicao.take')}>
        <section className="st-group">
          <h2 className="st-group__title">{t('studio.edicao.take')}</h2>
          <div className="st-card st-card--active">
            <div className="st-card__row">
              <span className="st-take__thumb" aria-hidden="true" />
              <div className="st-take__meta">
                <strong className="st-small">{titulo.trim() || t('studio.semTitulo')}</strong>
                <span className="dx-num st-small dx-muted">
                  {temDuracao ? mmss(dur) : '--:--'} · {t('studio.edicao.faixasCompletas')}
                </span>
              </div>
            </div>
          </div>
        </section>

        {resultado.faixas.audio && podeCortar && (
          <section className="st-group" data-studio="pausas">
            <h2 className="st-group__title">{t('studio.edicao.pausas.titulo')}</h2>
            <p className="st-note">{t('studio.edicao.pausas.nota')}</p>
            {!analise ? (
              <Button
                variant="secondary"
                icon="wand"
                busy={aAnalisar}
                disabled={aAnalisar || aTrabalhar}
                onClick={onProcurarPausas}
              >
                {aAnalisar ? t('studio.edicao.pausas.aAnalisar') : t('studio.edicao.pausas.procurar')}
              </Button>
            ) : r && r.pausas === 0 ? (
              <p className="st-note st-note--ok">{t('studio.edicao.pausas.nenhuma')}</p>
            ) : (
              r && (
                <div className="st-card st-card--active">
                  <p className="dx-num st-small">
                    {t('studio.edicao.pausas.encontradas', { n: r.pausas, s: r.poupanca, pct: r.pct })}
                  </p>
                  <div className="st-actions">
                    <Button variant="primary" size="sm" icon="scissors" disabled={aTrabalhar} onClick={onRemoverPausas}>
                      {aTrabalhar ? rotuloProgresso : t('studio.edicao.pausas.remover')}
                    </Button>
                    <Button variant="ghost" size="sm" disabled={aTrabalhar} onClick={onCancelarPausas}>
                      {t('studio.edicao.pausas.cancelar')}
                    </Button>
                  </div>
                </div>
              )
            )}
          </section>
        )}
      </aside>

      <div className="st-centre">
        <div className="st-preview">
          <video
            ref={previewRef}
            className="st-preview__video"
            src={resultado.url}
            controls
            playsInline
            data-studio="preview"
            onLoadedMetadata={(e) => lerDuracao(e.currentTarget)}
            onDurationChange={(e) => {
              const d = e.currentTarget.duration
              if (Number.isFinite(d) && d > 0) onDuracao(d)
            }}
          />
          <span className="st-overlay st-overlay--tl dx-num" aria-hidden="true">
            {t('studio.palco.previsualizacao')}
          </span>
        </div>
        <div className="st-tools" role="toolbar" aria-label={t('studio.edicao.ferramentas')}>
          {resultado.faixas.audio && (
            <>
              <span className="st-small dx-muted">{t('studio.edicao.faixas.titulo')}</span>
              <Button size="sm" variant="secondary" icon="download" disabled={!resultado.faixas.video} onClick={() => onDescarregar('video')}>
                {t('studio.edicao.faixas.video')}
              </Button>
              <Button size="sm" variant="secondary" icon="download" onClick={() => onDescarregar('audio')}>
                {t('studio.edicao.faixas.audio')}
              </Button>
            </>
          )}
        </div>
      </div>

      <aside className="st-col st-col--right" aria-label={t('studio.edicao.corte')}>
        <section className="st-group">
          <h2 className="st-group__title">{t('studio.edicao.corte')}</h2>
          {!podeCortar ? (
            <Alert tone="warning">{t('studio.edicao.semWebCodecs')}</Alert>
          ) : !temDuracao ? (
            <p className="st-note">--:--</p>
          ) : (
            <div className="st-trim" data-studio="corte">
              <div className="st-trim__io">
                <div>
                  <span className="st-label">{t('studio.edicao.entrada')}</span>
                  <span className="st-timecode dx-num">{mmss(de)}</span>
                </div>
                <div>
                  <span className="st-label">{t('studio.edicao.saida')}</span>
                  <span className="st-timecode st-timecode--on dx-num">{mmss(ate)}</span>
                </div>
              </div>
              <label className="st-label" htmlFor="st-corte-de">
                {t('studio.edicao.cursorEntrada')}
              </label>
              <input
                id="st-corte-de"
                className="st-range"
                type="range"
                min={0}
                max={max}
                value={Math.min(de, ate)}
                data-studio="corte-de"
                onChange={(e) => {
                  const v = Number(e.target.value)
                  onDe(v)
                  buscar(v)
                }}
              />
              <label className="st-label" htmlFor="st-corte-ate">
                {t('studio.edicao.cursorSaida')}
              </label>
              <input
                id="st-corte-ate"
                className="st-range"
                type="range"
                min={0}
                max={max}
                value={ate}
                data-studio="corte-ate"
                onChange={(e) => {
                  const v = Number(e.target.value)
                  onAte(v)
                  buscar(v)
                }}
              />
              <Button
                variant="primary"
                icon="scissors"
                data-studio="cortar"
                disabled={aTrabalhar || ate - de < 1 || corteInteiro}
                onClick={onCortar}
              >
                {aTrabalhar ? rotuloProgresso : `${t('studio.edicao.cortar')} · ${mmss(ate - de)}`}
              </Button>
            </div>
          )}
        </section>

        <section className="st-group" data-studio="guardar">
          <h2 className="st-group__title">{t('studio.edicao.destino.titulo')}</h2>
          <Field label={t('studio.edicao.destino.campoTitulo')} htmlFor="st-titulo">
            <TextInput
              id="st-titulo"
              value={titulo}
              maxLength={80}
              placeholder={t('studio.edicao.destino.tituloPh')}
              onChange={(e) => onTitulo(e.target.value)}
            />
          </Field>
          <div className="st-actions">
            <Button variant="primary" icon="upload" busy={aGuardar} disabled={aGuardar} data-studio="guardar-biblioteca" onClick={onGuardar}>
              {aGuardar ? t('studio.edicao.destino.aGuardar') : t('studio.edicao.destino.guardar')}
            </Button>
            <Button variant="secondary" icon="download" onClick={() => onDescarregar('completo')}>
              {t('studio.edicao.destino.descarregar')}
            </Button>
          </div>
          {guardado && (
            <p className="st-note st-note--ok" role="status" data-studio="guardado">
              {guardado}
            </p>
          )}
        </section>
      </aside>

      <section className="st-timeline" aria-label={t('studio.edicao.linhaTempo')}>
        <div className="st-timeline__head">
          <span className="dx-num st-timecode">{mmss(agora)}</span>
          <span className="dx-num st-small dx-muted">/ {temDuracao ? mmss(dur) : '--:--'}</span>
        </div>
        <div className="st-timeline__tracks">
          <span className="st-track__label dx-num">{t('studio.edicao.pistaVideo')}</span>
          <div
            className="st-track"
            onClick={(e) => {
              if (!temDuracao) return
              const b = e.currentTarget.getBoundingClientRect()
              buscar(Math.min(1, Math.max(0, (e.clientX - b.left) / b.width)) * dur)
            }}
          >
            <span className="st-clip" style={{ left: pct(de), width: `calc(${pct(ate)} - ${pct(de)})` }} />
            <span className="st-playhead" style={{ left: pct(agora) }} aria-hidden="true" />
          </div>
          {resultado.faixas.audio && (
            <>
              <span className="st-track__label dx-num">{t('studio.edicao.pistaAudio')}</span>
              <div className="st-track st-track--audio">
                {analise?.pausas.map((p, i) => (
                  <span
                    key={i}
                    className="st-gap"
                    style={{
                      left: `${(p.inicio / analise.duracao) * 100}%`,
                      width: `${((p.fim - p.inicio) / analise.duracao) * 100}%`,
                    }}
                  />
                ))}
                <span className="st-playhead" style={{ left: pct(agora) }} aria-hidden="true" />
              </div>
            </>
          )}
        </div>
      </section>
      <span className="dx-sr-only" aria-live="polite">
        {aTrabalhar ? rotuloProgresso : ''}
      </span>
    </div>
  )
}
