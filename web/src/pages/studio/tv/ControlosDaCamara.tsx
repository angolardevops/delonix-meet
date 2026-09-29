/**
 * Controlos de uma câmara pelo que o BROWSER deixa: `getCapabilities()` diz o
 * que o dispositivo expõe e `applyConstraints()` aplica. Um controlo que o
 * dispositivo não expõe aparece desligado com a razão escrita — não se finge
 * um ISO que a câmara não aceita.
 *
 * Serve a câmara local nas Fontes e a câmara do telefone no PhoneCam.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Deslizador } from './pecas'

/** As capacidades de «image capture» que o Chromium expõe em `MediaTrackCapabilities`. */
export interface CapacidadesDaCamara {
  exposureMode?: string[]
  exposureCompensation?: { min: number; max: number; step: number }
  colorTemperature?: { min: number; max: number; step: number }
  whiteBalanceMode?: string[]
  iso?: { min: number; max: number; step: number }
  focusMode?: string[]
  focusDistance?: { min: number; max: number; step: number }
  zoom?: { min: number; max: number; step: number }
}

type Definicoes = MediaTrackSettings & {
  exposureCompensation?: number
  colorTemperature?: number
  iso?: number
  focusDistance?: number
  zoom?: number
  exposureMode?: string
  focusMode?: string
}

export function capacidades(track: MediaStreamTrack | null): CapacidadesDaCamara {
  try {
    return ((track as MediaStreamTrack & { getCapabilities?: () => unknown })?.getCapabilities?.() ?? {}) as CapacidadesDaCamara
  } catch {
    return {}
  }
}

export async function aplicar(track: MediaStreamTrack, c: Record<string, unknown>): Promise<boolean> {
  try {
    await track.applyConstraints({ advanced: [c as MediaTrackConstraintSet] })
    return true
  } catch {
    return false
  }
}

export type ControloDaCamara = 'exposicao' | 'temperatura' | 'iso' | 'foco' | 'zoom'

export function ControlosDaCamara({
  track,
  controlos = ['iso', 'foco', 'zoom', 'temperatura', 'exposicao'],
  larguraRotulo = 56,
  larguraValor = 54,
}: {
  track: MediaStreamTrack | null
  controlos?: ControloDaCamara[]
  larguraRotulo?: number
  larguraValor?: number
}) {
  const { t, i18n } = useTranslation()
  const lang = i18n.language
  const caps = capacidades(track)
  const [def, setDef] = useState<Definicoes>(() => (track?.getSettings() ?? {}) as Definicoes)
  const [erro, setErro] = useState('')
  useEffect(() => setDef((track?.getSettings() ?? {}) as Definicoes), [track])

  const mudar = async (c: Record<string, unknown>) => {
    if (!track) return
    setErro('')
    const ok = await aplicar(track, c)
    if (!ok) setErro(t('tv.camara.recusado'))
    setDef({ ...(track.getSettings() as Definicoes) })
  }
  const semSuporte = t('tv.camara.semSuporte')
  const num = (v: number | undefined, casas = 1) => (v === undefined ? '—' : v.toLocaleString(lang, { maximumFractionDigits: casas }))

  const linha = (k: ControloDaCamara) => {
    switch (k) {
      case 'exposicao': {
        const r = caps.exposureCompensation
        return (
          <Deslizador
            key={k}
            rotulo={t('tv.camara.exposicao')}
            valor={def.exposureCompensation ?? 0}
            min={r?.min ?? -2}
            max={r?.max ?? 2}
            passo={r?.step || 0.1}
            disabled={!r}
            title={r ? undefined : semSuporte}
            texto={r ? t('tv.unidades.ev', { v: num(def.exposureCompensation) }) : '—'}
            onChange={(v) => void mudar({ exposureMode: 'continuous', exposureCompensation: v })}
            larguraRotulo={larguraRotulo}
            larguraValor={larguraValor}
          />
        )
      }
      case 'temperatura': {
        const r = caps.colorTemperature
        return (
          <Deslizador
            key={k}
            rotulo={t('tv.camara.branco')}
            valor={def.colorTemperature ?? 5200}
            min={r?.min ?? 2500}
            max={r?.max ?? 7500}
            passo={r?.step || 50}
            disabled={!r}
            title={r ? undefined : semSuporte}
            texto={r ? t('tv.unidades.kelvin', { v: num(def.colorTemperature, 0) }) : '—'}
            onChange={(v) => void mudar({ whiteBalanceMode: 'manual', colorTemperature: v })}
            larguraRotulo={larguraRotulo}
            larguraValor={larguraValor}
          />
        )
      }
      case 'iso': {
        const r = caps.iso
        return (
          <Deslizador
            key={k}
            rotulo={t('tv.camara.iso')}
            valor={def.iso ?? 100}
            min={r?.min ?? 50}
            max={r?.max ?? 3200}
            passo={r?.step || 1}
            disabled={!r}
            title={r ? undefined : semSuporte}
            texto={r ? num(def.iso, 0) : '—'}
            onChange={(v) => void mudar({ exposureMode: 'manual', iso: v })}
            larguraRotulo={larguraRotulo}
            larguraValor={larguraValor}
          />
        )
      }
      case 'foco': {
        const r = caps.focusDistance
        return (
          <Deslizador
            key={k}
            rotulo={t('tv.camara.foco')}
            valor={def.focusDistance ?? 1}
            min={r?.min ?? 0}
            max={r?.max ?? 10}
            passo={r?.step || 0.01}
            disabled={!r}
            title={r ? undefined : semSuporte}
            texto={r ? t('tv.unidades.metros', { v: num(def.focusDistance) }) : '—'}
            onChange={(v) => void mudar({ focusMode: 'manual', focusDistance: v })}
            larguraRotulo={larguraRotulo}
            larguraValor={larguraValor}
          />
        )
      }
      case 'zoom': {
        const r = caps.zoom
        return (
          <Deslizador
            key={k}
            rotulo={t('tv.camara.zoom')}
            valor={def.zoom ?? 1}
            min={r?.min ?? 1}
            max={r?.max ?? 4}
            passo={r?.step || 0.1}
            disabled={!r}
            title={r ? undefined : semSuporte}
            texto={r ? t('tv.unidades.vezes', { v: num(def.zoom) }) : '—'}
            onChange={(v) => void mudar({ zoom: v })}
            larguraRotulo={larguraRotulo}
            larguraValor={larguraValor}
          />
        )
      }
    }
  }

  return (
    <>
      {controlos.map(linha)}
      {erro && <p className="tv-erro">{erro}</p>}
    </>
  )
}

/** Bloquear exposição e foco (modo manual nos dois), se o dispositivo deixar. */
export function podeBloquear(track: MediaStreamTrack | null): boolean {
  const c = capacidades(track)
  return !!(c.exposureMode?.includes('manual') || c.focusMode?.includes('manual'))
}

export async function bloquearAeAf(track: MediaStreamTrack, bloquear: boolean): Promise<boolean> {
  const c = capacidades(track)
  const pedido: Record<string, unknown> = {}
  if (c.exposureMode?.includes(bloquear ? 'manual' : 'continuous')) pedido.exposureMode = bloquear ? 'manual' : 'continuous'
  if (c.focusMode?.includes(bloquear ? 'manual' : 'continuous')) pedido.focusMode = bloquear ? 'manual' : 'continuous'
  if (!Object.keys(pedido).length) return false
  return aplicar(track, pedido)
}

/** Focar uma vez (o «focar no rosto» possível sem detecção: foco automático de disparo único). */
export function podeFocarUmaVez(track: MediaStreamTrack | null): boolean {
  return !!capacidades(track).focusMode?.includes('single-shot')
}
