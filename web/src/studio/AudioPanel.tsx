/**
 * Áudio do palco: o microfone e os três faders (Palco, Música, Vídeo). Cada
 * fader é um `GainNode` no compositor — mexer nele não reconstrói o grafo nem
 * corta a gravação. A música é um ficheiro do dispositivo, em ciclo.
 */
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, Select } from '../ui/kit'
import type { Mistura } from './palco'
import type { Microfone } from './usePalco'

const FADERS: (keyof Mistura)[] = ['palco', 'musica', 'video']

export default function AudioPanel({
  mistura,
  microfones,
  microfone,
  musica,
  musicaATocar,
  onMistura,
  onMicrofone,
  onMusica,
  onAlternarMusica,
}: {
  mistura: Mistura
  microfones: Microfone[]
  microfone: string
  musica: { nome: string } | null
  musicaATocar: boolean
  onMistura: (patch: Partial<Mistura>) => void
  onMicrofone: (id: string) => void
  onMusica: (f: File | null) => void
  onAlternarMusica: () => void
}) {
  const { t } = useTranslation()
  const ficheiro = useRef<HTMLInputElement>(null)
  return (
    <section className="st-group" data-studio-grupo="audio" aria-labelledby="st-audio-h">
      <h2 id="st-audio-h" className="st-group__title">
        {t('studio.audio.titulo')}
      </h2>

      <label className="st-label" htmlFor="st-mic">
        {t('studio.audio.microfone')}
      </label>
      <Select id="st-mic" value={microfone} data-studio="microfone" onChange={(e) => onMicrofone(e.target.value)}>
        <option value="">{t('studio.audio.microfoneOmissao')}</option>
        {microfones
          .filter((m) => m.id && m.id !== 'default')
          .map((m) => (
            <option key={m.id} value={m.id}>
              {m.nome}
            </option>
          ))}
      </Select>

      <div className="st-faders">
        {FADERS.map((f) => (
          <label key={f} className="st-fader">
            <span className="st-fader__name">{t(`studio.audio.faders.${f}`)}</span>
            <input
              type="range"
              className="st-range"
              min={0}
              max={150}
              step={1}
              value={Math.round(mistura[f] * 100)}
              aria-valuetext={`${Math.round(mistura[f] * 100)}%`}
              data-studio-fader={f}
              onChange={(e) => onMistura({ [f]: Number(e.target.value) / 100 })}
            />
            <span className="dx-num st-fader__val">{Math.round(mistura[f] * 100)}%</span>
          </label>
        ))}
      </div>

      <input
        ref={ficheiro}
        type="file"
        accept="audio/*"
        hidden
        onChange={(e) => {
          onMusica(e.target.files?.[0] ?? null)
          e.target.value = ''
        }}
      />
      {musica ? (
        <div className="st-card">
          <div className="st-card__row">
            <span className="st-small st-ellipsis" title={musica.nome}>
              {musica.nome}
            </span>
          </div>
          <div className="st-actions">
            <Button size="sm" variant="secondary" icon={musicaATocar ? 'pause' : 'play'} data-studio="musica" onClick={onAlternarMusica}>
              {musicaATocar ? t('studio.audio.pausarMusica') : t('studio.audio.tocarMusica')}
            </Button>
            <Button size="sm" variant="ghost" icon="x" onClick={() => onMusica(null)}>
              {t('studio.audio.tirarMusica')}
            </Button>
          </div>
        </div>
      ) : (
        <Button size="sm" variant="outline" icon="upload" onClick={() => ficheiro.current?.click()}>
          {t('studio.audio.carregarMusica')}
        </Button>
      )}
      <p className="st-note">{t('studio.audio.nota')}</p>
    </section>
  )
}
