import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import { DelonixSymbol, Icon } from '../ui/icons'
import { Alert, Button, Checkbox, IconButton, cx } from '../ui/kit'
import { MicLevel } from './MicLevel'
import { TileAvatar } from './ParticipantTile'
import type { LocalMedia } from './useLocalMedia'
import type { usePrejoin } from './usePrejoin'

function DeviceOption({
  selected,
  label,
  meta,
  onSelect,
  children,
}: {
  selected: boolean
  label: string
  meta?: string
  onSelect: () => void
  children?: ReactNode
}) {
  return (
    <div className={cx('rm-devopt', selected && 'is-on')}>
      <button type="button" role="radio" aria-checked={selected} className="rm-devopt__btn" onClick={onSelect}>
        <span className="rm-devopt__check" aria-hidden="true">
          {selected && <Icon name="check" size={10} />}
        </span>
        <span className="rm-devopt__text">
          <span className="rm-devopt__label">{label}</span>
          {meta && <span className="rm-devopt__meta dx-num">{meta}</span>}
        </span>
      </button>
      {children}
    </div>
  )
}

/**
 * Pré-entrada: ver-se, ouvir-se e escolher dispositivos ANTES de entrar. Só
 * mostra fontes que existem: uma câmara activa, os microfones e as saídas que
 * o browser enumera.
 */
export function Prejoin({
  code,
  media,
  prejoin,
  status,
  onJoin,
  onCancel,
}: {
  code: string
  media: LocalMedia
  prejoin: ReturnType<typeof usePrejoin>
  status: string
  onJoin: (audioOnly: boolean) => void
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const me = currentUser()?.username ?? ''
  const { devices } = media
  const showVideo = media.hasLocalVideo && media.camOn
  const semMedia = !prejoin.previewStream?.getTracks().length

  return (
    <div className="rm-prejoin">
      <header className="rm-top">
        <span className="rm-top__mark" aria-hidden="true">
          <DelonixSymbol size={18} />
        </span>
        <h1 className="rm-top__title">{t('room.preEntrada.titulo')}</h1>
        <span className="rm-top__meta dx-num">{t('room.preEntrada.sala', { code })}</span>
        <span className="dx-spacer" />
        <Button size="sm" variant="ghost" icon="door" className="rm-hide-narrow" onClick={() => (location.hash = `/lobby/${code}`)}>
          {t('room.preEntrada.gerirSalaDeEspera')}
        </Button>
        <IconButton
          icon="door"
          label={t('room.preEntrada.gerirSalaDeEspera')}
          className="rm-only-narrow"
          onClick={() => (location.hash = `/lobby/${code}`)}
        />
        <Button size="sm" variant="outline" onClick={onCancel}>
          {t('room.preEntrada.cancelar')}
        </Button>
      </header>

      <div className="rm-prejoin__body">
        <main className="rm-prejoin__main">
          <div className="rm-prejoin__heading">
            <h2>{t('room.preEntrada.verificarDispositivos')}</h2>
            <span className="dx-num dx-muted">{showVideo ? t('room.preEntrada.umaCamaraActiva') : t('room.preEntrada.semVideo')}</span>
          </div>

          <div className="rm-prejoin__preview" style={{ ['--tone' as string]: 'var(--accent-strong)' }}>
            {media.hasLocalVideo && (
              <video
                key={prejoin.previewVersion}
                ref={prejoin.attachPreview}
                autoPlay
                playsInline
                muted
                className={cx('rm-prejoin__video', !showVideo && 'is-hidden')}
              />
            )}
            {!showVideo && (
              <div className="rm-prejoin__novideo">
                <TileAvatar name={me || code} />
                <span className="dx-muted">{media.hasLocalVideo ? t('room.preEntrada.camaraDesligada') : t('room.preEntrada.semCamara')}</span>
              </div>
            )}
            <div className="rm-prejoin__tags">
              <span className="rm-flag">{showVideo ? t('room.preEntrada.fonteCamara') : t('room.preEntrada.fonteAudio')}</span>
            </div>
            <div className="rm-prejoin__foot">
              <span className="rm-flag rm-flag--name">{me ? t('room.preEntrada.tuNome', { nome: me }) : t('room.preEntrada.tu')}</span>
              <span className="dx-spacer" />
              <div className="rm-prejoin__toggles">
                <button
                  type="button"
                  className={cx('rm-round', !media.micOn && 'is-off')}
                  onClick={() => prejoin.toggle('mic')}
                  disabled={!prejoin.previewStream?.getAudioTracks().length}
                  aria-pressed={!media.micOn}
                  aria-label={media.micOn ? t('room.controlos.desligarMicrofone') : t('room.controlos.ligarMicrofone')}
                  title={media.micOn ? t('room.controlos.desligarMicrofone') : t('room.controlos.ligarMicrofone')}
                >
                  <Icon name={media.micOn ? 'mic' : 'micOff'} />
                </button>
                <button
                  type="button"
                  className={cx('rm-round', !showVideo && 'is-off')}
                  onClick={() => prejoin.toggle('cam')}
                  disabled={!prejoin.previewStream?.getVideoTracks().length}
                  aria-pressed={!media.camOn}
                  aria-label={media.camOn ? t('room.controlos.desligarCamara') : t('room.controlos.ligarCamara')}
                  title={media.camOn ? t('room.controlos.desligarCamara') : t('room.controlos.ligarCamara')}
                >
                  <Icon name={showVideo ? 'video' : 'videoOff'} />
                </button>
              </div>
            </div>
          </div>

          {status && <Alert tone={semMedia ? 'warning' : undefined}>{status}</Alert>}
        </main>

        <aside className="rm-prejoin__side">
          <section className="rm-prejoin__group" aria-labelledby="rm-pj-cams">
            <h3 id="rm-pj-cams" className="rm-prejoin__label">
              {t('room.preEntrada.camaras')}
            </h3>
            <div role="radiogroup" aria-labelledby="rm-pj-cams" className="rm-prejoin__list">
              {devices.cams.length === 0 && <p className="dx-muted rm-prejoin__empty">{t('room.preEntrada.nenhumaCamara')}</p>}
              {devices.cams.map((d, i) => (
                <DeviceOption
                  key={d.deviceId || i}
                  selected={d.deviceId === media.camId}
                  label={d.label || t('room.preEntrada.camaraN', { n: i + 1 })}
                  meta={d.deviceId === media.camId ? t('room.preEntrada.emUso') : undefined}
                  onSelect={() => void prejoin.switchDevice('cam', d.deviceId)}
                />
              ))}
            </div>
          </section>

          <section className="rm-prejoin__group" aria-labelledby="rm-pj-mics">
            <h3 id="rm-pj-mics" className="rm-prejoin__label">
              {t('room.preEntrada.microfones')}
            </h3>
            <div role="radiogroup" aria-labelledby="rm-pj-mics" className="rm-prejoin__list">
              {devices.mics.length === 0 && <p className="dx-muted rm-prejoin__empty">{t('room.preEntrada.nenhumMicrofone')}</p>}
              {devices.mics.map((d, i) => (
                <DeviceOption
                  key={d.deviceId || i}
                  selected={d.deviceId === media.micId}
                  label={d.label || t('room.preEntrada.microfoneN', { n: i + 1 })}
                  onSelect={() => void prejoin.switchDevice('mic', d.deviceId)}
                >
                  {d.deviceId === media.micId && <MicLevel stream={prejoin.previewStream} version={prejoin.previewVersion} />}
                </DeviceOption>
              ))}
            </div>
            <Checkbox
              label={t('room.preEntrada.supressaoRuido')}
              checked={media.noiseSuppression}
              onChange={(e) => media.setNoiseSuppression(e.target.checked)}
            />
          </section>

          <section className="rm-prejoin__group" aria-labelledby="rm-pj-out">
            <h3 id="rm-pj-out" className="rm-prejoin__label">
              {t('room.preEntrada.saidaAudio')}
            </h3>
            <div role="radiogroup" aria-labelledby="rm-pj-out" className="rm-prejoin__list">
              <DeviceOption
                selected={media.speakerId === ''}
                label={t('room.preEntrada.predefinidoSistema')}
                onSelect={() => media.setSpeakerId('')}
              />
              {devices.speakers
                .filter((d) => d.deviceId && d.deviceId !== 'default')
                .map((d, i) => (
                  <DeviceOption
                    key={d.deviceId}
                    selected={d.deviceId === media.speakerId}
                    label={d.label || t('room.preEntrada.altifalanteN', { n: i + 1 })}
                    onSelect={() => media.setSpeakerId(d.deviceId)}
                  />
                ))}
            </div>
            <Button size="sm" variant="outline" icon="volume" onClick={media.testSpeaker}>
              {t('room.preEntrada.testarSom')}
            </Button>
          </section>

          <div className="rm-prejoin__cta">
            <Button variant="primary" size="lg" block icon="video" onClick={() => onJoin(false)}>
              {t('room.preEntrada.entrar')}
            </Button>
            {media.hasLocalVideo && (
              <Button variant="ghost" size="sm" block icon="mic" onClick={() => onJoin(true)}>
                {t('room.preEntrada.entrarSoAudio')}
              </Button>
            )}
          </div>
        </aside>
      </div>
    </div>
  )
}
