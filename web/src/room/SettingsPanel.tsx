import { ReactNode, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Button, Field, Select, Toggle, cx } from '../ui/kit'
import { VolumeSlider } from './Prejoin'
import type { LocalMedia } from './useLocalMedia'
import type { Transcription } from './useTranscription'

const CC_LANGS = ['', 'pt', 'en', 'fr', 'es', 'de'] as const

/** Dispositivos, efeitos de fundo e legendas — tudo o que só afecta este dispositivo. */
export function SettingsPanel({
  media,
  transcription,
  localVideo,
  children,
}: {
  media: LocalMedia
  transcription: Transcription
  localVideo: HTMLVideoElement | null
  /** Secções extra no fim (nitidez e palco imersivo — `room/enhance`). */
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const previewRef = useRef<HTMLVideoElement>(null)
  const uploadRef = useRef<HTMLInputElement>(null)

  // A pré-visualização mostra o MESMO stream que os outros vêem (com efeito).
  useEffect(() => {
    if (previewRef.current && localVideo) previewRef.current.srcObject = localVideo.srcObject
  }, [localVideo, media.bgMode, media.bgImageUrl, media.hasLocalVideo, media.camOn])

  const semCamara = !media.hasLocalVideo
  const busy = media.bgBusy || semCamara
  return (
    <div className="rm-scroll">
      <section className="rm-block" aria-labelledby="rm-fx-h">
        <h3 id="rm-fx-h" className="rm-block__title">
          <Icon name="sparkles" size={13} />
          {t('room.definicoes.fundos')}
          <span className="dx-spacer" />
          {media.bgBusy && <span className="dx-muted">{t('room.definicoes.aAplicar')}</span>}
        </h3>
        <div className="rm-fx__preview">
          <video ref={previewRef} autoPlay muted playsInline className={cx(media.bgMode === 'none' && 'is-mirror')} />
        </div>
        <p className="dx-muted">{t('room.definicoes.iaLocal')}</p>
        {semCamara && <Alert tone="warning">{t('room.definicoes.ligaCamara')}</Alert>}
        <div className="rm-fx__grid" role="radiogroup" aria-label={t('room.definicoes.fundos')}>
          <button
            type="button"
            role="radio"
            aria-checked={media.bgMode === 'none'}
            className={cx('rm-fx__opt', media.bgMode === 'none' && 'is-on')}
            disabled={busy}
            onClick={() => void media.applyBackground('none')}
          >
            <Icon name="ban" />
            <span>{t('room.definicoes.semEfeito')}</span>
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={media.bgMode === 'blur' && media.blurLevel === 'light'}
            className={cx('rm-fx__opt', media.bgMode === 'blur' && media.blurLevel === 'light' && 'is-on')}
            disabled={busy}
            onClick={() => void media.applyBackground('blur', undefined, 'light')}
          >
            <Icon name="blur" />
            <span>{t('room.definicoes.desfoqueLeve')}</span>
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={media.bgMode === 'blur' && media.blurLevel === 'strong'}
            className={cx('rm-fx__opt', media.bgMode === 'blur' && media.blurLevel === 'strong' && 'is-on')}
            disabled={busy}
            onClick={() => void media.applyBackground('blur', undefined, 'strong')}
          >
            <Icon name="blur" />
            <span>{t('room.definicoes.desfoqueForte')}</span>
          </button>
          {media.presets.map((p) => (
            <button
              key={p.url}
              type="button"
              role="radio"
              aria-checked={media.bgMode === 'image' && media.bgImageUrl === p.url}
              className={cx('rm-fx__opt rm-fx__img', media.bgMode === 'image' && media.bgImageUrl === p.url && 'is-on')}
              disabled={busy}
              onClick={() => void media.applyBackground('image', p.url)}
            >
              <img src={p.url} alt="" />
              <span>{p.name}</span>
            </button>
          ))}
          <button type="button" className="rm-fx__opt" disabled={busy} onClick={() => uploadRef.current?.click()}>
            <Icon name="upload" />
            <span>{t('room.definicoes.carregarImagem')}</span>
          </button>
        </div>
        <input
          ref={uploadRef}
          type="file"
          accept="image/*"
          hidden
          onChange={(e) => {
            media.uploadBackground(e.target.files?.[0] ?? null)
            e.target.value = ''
          }}
        />
      </section>

      <section className="rm-block" aria-labelledby="rm-dev-h">
        <h3 id="rm-dev-h" className="rm-block__title">
          <Icon name="sliders" size={13} />
          {t('room.definicoes.dispositivos')}
        </h3>
        <Field label={t('room.definicoes.microfone')} htmlFor="rm-set-mic">
          <Select id="rm-set-mic" value={media.micId} onChange={(e) => void media.switchMic(e.target.value)}>
            {media.devices.mics.length === 0 && <option value="">{t('room.definicoes.semDispositivos')}</option>}
            {media.devices.mics.map((d, i) => (
              <option key={d.deviceId || i} value={d.deviceId}>
                {d.label || t('room.preEntrada.microfoneN', { n: i + 1 })}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={t('room.definicoes.camara')} htmlFor="rm-set-cam">
          <Select id="rm-set-cam" value={media.camId} onChange={(e) => void media.switchCam(e.target.value)}>
            {media.devices.cams.length === 0 && <option value="">{t('room.definicoes.semDispositivos')}</option>}
            {media.devices.cams.map((d, i) => (
              <option key={d.deviceId || i} value={d.deviceId}>
                {d.label || t('room.preEntrada.camaraN', { n: i + 1 })}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={t('room.definicoes.altifalantes')} htmlFor="rm-set-spk">
          <Select id="rm-set-spk" value={media.speakerId} onChange={(e) => media.setSpeakerId(e.target.value)}>
            <option value="">{t('room.preEntrada.predefinidoSistema')}</option>
            {media.devices.speakers
              .filter((d) => d.deviceId && d.deviceId !== 'default')
              .map((d, i) => (
                <option key={d.deviceId} value={d.deviceId}>
                  {d.label || t('room.preEntrada.altifalanteN', { n: i + 1 })}
                </option>
              ))}
          </Select>
        </Field>
        <VolumeSlider media={media} id="rm-set-vol" />
        <Button size="sm" variant="outline" icon="volume" onClick={media.testSpeaker}>
          {t('room.preEntrada.testarSom')}
        </Button>
        <Toggle
          label={t('room.definicoes.supressaoRuido')}
          hint={t('room.definicoes.supressaoRuidoDica')}
          checked={media.noiseSuppression}
          onChange={() => void media.toggleNoiseSuppression()}
        />
      </section>

      <section className="rm-block" aria-labelledby="rm-cc-h">
        <h3 id="rm-cc-h" className="rm-block__title">
          <Icon name="captions" size={13} />
          {t('room.definicoes.legendas')}
        </h3>
        <Field label={t('room.definicoes.traduzirLegendas')} htmlFor="rm-set-cc" hint={t('room.definicoes.traduzirDica')}>
          <Select id="rm-set-cc" value={transcription.ccLang} onChange={(e) => transcription.setCcLang(e.target.value)}>
            {CC_LANGS.map((l) => (
              <option key={l} value={l}>
                {t(`room.definicoes.idioma_${l || 'original'}`)}
              </option>
            ))}
          </Select>
        </Field>
        <Toggle
          label={t('room.definicoes.transcricaoServidor')}
          hint={t('room.definicoes.transcricaoServidorDica')}
          checked={transcription.serverAsr}
          onChange={(e) => transcription.setServerAsr(e.target.checked)}
        />
      </section>
      {children}
    </div>
  )
}
