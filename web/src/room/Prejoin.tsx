import { ReactNode, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import { DelonixSymbol, Icon } from '../ui/icons'
import { Alert, Button, Checkbox, IconButton, cx } from '../ui/kit'
import { metaCurta, metaLonga, videoMeta } from './mediaMeta'
import { MicLevel } from './MicLevel'
import { TileAvatar } from './ParticipantTile'
import type { LocalMedia } from './useLocalMedia'
import type { Prejoin as PrejoinState } from './usePrejoin'

/** Dispositivos que o browser inventa por cima dos reais: misturá-los duplicava o mesmo microfone. */
const PSEUDO = new Set(['', 'default', 'communications'])

function DeviceOption({
  selected,
  label,
  meta,
  onSelect,
  extra,
  children,
}: {
  selected: boolean
  label: string
  meta?: string
  onSelect: () => void
  /** Acção secundária da linha (usar como fonte 2, misturar). */
  extra?: ReactNode
  children?: ReactNode
}) {
  return (
    <div className={cx('rm-devopt', selected && 'is-on')}>
      <div className="rm-devopt__line">
        <button type="button" role="radio" aria-checked={selected} className="rm-devopt__btn" onClick={onSelect}>
          <span className="rm-devopt__check" aria-hidden="true">
            {selected && <Icon name="check" size={10} />}
          </span>
          <span className="rm-devopt__text">
            <span className="rm-devopt__label">{label}</span>
            {meta && <span className="rm-devopt__meta dx-num">{meta}</span>}
          </span>
        </button>
        {extra}
      </div>
      {children}
    </div>
  )
}

/**
 * Pré-entrada: ver-se, ouvir-se e escolher dispositivos ANTES de entrar. Só
 * mostra fontes que existem: as câmaras e microfones que o browser enumera, e
 * os números que as próprias tracks declaram.
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
  prejoin: PrejoinState
  status: string
  onJoin: (audioOnly: boolean) => void
  onCancel: () => void
}) {
  const { t, i18n } = useTranslation()
  const me = currentUser()?.username ?? ''
  const { devices } = media
  const { info } = prejoin
  const [picker, setPicker] = useState(false)
  const uploadRef = useRef<HTMLInputElement>(null)
  const showVideo = media.hasLocalVideo && media.camOn
  const semMedia = !prejoin.previewStream?.getTracks().length
  const locale = i18n.language === 'en' ? 'en-GB' : i18n.language === 'fr' ? 'fr-FR' : 'pt-PT'

  // Os números vêm das tracks (o que a câmara ENTREGA), não do que se pediu.
  const camMeta = videoMeta(prejoin.previewStream?.getVideoTracks()[0])
  const secondMeta = videoMeta(prejoin.second?.stream.getVideoTracks()[0])
  const fontes = (showVideo ? 1 : 0) + (prejoin.second ? 1 : 0)
  const podeSegundaFonte = info?.topology === 'sfu' && media.hasLocalVideo && devices.cams.length > 1
  const podeMisturar =
    !!prejoin.previewStream?.getAudioTracks().length && devices.mics.filter((d) => !PSEUDO.has(d.deviceId)).length > 1
  const hora = info?.startsAt
    ? new Date(info.startsAt).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit', timeZoneName: 'short' })
    : null
  const nomeTag = me
    ? info?.owner
      ? t('room.preEntrada.nomePapel', { nome: me, papel: t('room.papel.anfitriao') })
      : t('room.preEntrada.tuNome', { nome: me })
    : t('room.preEntrada.tu')
  const segundaLabel = devices.cams.find((d) => d.deviceId === prejoin.second?.deviceId)?.label || t('room.preEntrada.camaraN', { n: 2 })

  return (
    <div className="rm-prejoin">
      <header className="rm-top">
        <span className="rm-top__mark" aria-hidden="true">
          <DelonixSymbol size={18} />
        </span>
        <h1 className="rm-top__title">{info?.name || t('room.preEntrada.titulo')}</h1>
        <span className="rm-top__meta dx-num">{hora ? t('room.preEntrada.horaSala', { hora, code }) : t('room.preEntrada.sala', { code })}</span>
        <span className="dx-spacer" />
        {info?.owner && (
          <>
            <Button size="sm" variant="ghost" icon="door" className="rm-hide-narrow" onClick={() => (location.hash = `/lobby/${code}`)}>
              {t('room.preEntrada.gerirSalaDeEspera')}
            </Button>
            <IconButton
              icon="door"
              label={t('room.preEntrada.gerirSalaDeEspera')}
              className="rm-only-narrow"
              onClick={() => (location.hash = `/lobby/${code}`)}
            />
          </>
        )}
        <Button size="sm" variant="outline" onClick={onCancel}>
          {t('room.preEntrada.cancelar')}
        </Button>
      </header>

      <div className="rm-prejoin__body">
        <main className="rm-prejoin__main">
          <div className="rm-prejoin__heading">
            <h2>{t('room.preEntrada.verificarDispositivos')}</h2>
            <span className="dx-num dx-muted">{fontes > 0 ? t('room.preEntrada.fontesActivas', { count: fontes }) : t('room.preEntrada.semVideo')}</span>
          </div>

          <div className={cx('rm-prejoin__stage', media.hasLocalVideo && 'has-second')}>
            <div className="rm-prejoin__preview" style={{ ['--tone' as string]: 'var(--accent-strong)' }}>
              {media.hasLocalVideo && (
                <video
                  key={prejoin.previewVersion}
                  ref={prejoin.attachPreview}
                  autoPlay
                  playsInline
                  muted
                  className={cx('rm-prejoin__video', !showVideo && 'is-hidden', media.bgMode !== 'none' && 'is-fx')}
                />
              )}
              {!showVideo && (
                <div className="rm-prejoin__novideo">
                  <TileAvatar name={me || code} />
                  <span className="dx-muted">{media.hasLocalVideo ? t('room.preEntrada.camaraDesligada') : t('room.preEntrada.semCamara')}</span>
                </div>
              )}
              <div className="rm-prejoin__tags">
                <span className="rm-flag">
                  {showVideo
                    ? prejoin.second
                      ? t('room.preEntrada.fonteN', { n: 1, tipo: t('room.preEntrada.fonteCamara') })
                      : t('room.preEntrada.fonteCamara')
                    : t('room.preEntrada.fonteAudio')}
                </span>
                {showVideo && camMeta && <span className="rm-flag rm-flag--meta">{metaLonga(camMeta, t('room.preEntrada.fps'))}</span>}
              </div>
              <div className="rm-prejoin__foot">
                <span className="rm-flag rm-flag--name">{nomeTag}</span>
                <span className="dx-spacer" />
                {showVideo && (
                  <div className="rm-prejoin__fx" role="group" aria-label={t('room.preEntrada.fundo')}>
                    <button
                      type="button"
                      className={cx('rm-fxchip', media.bgMode === 'blur' && 'is-on')}
                      aria-pressed={media.bgMode === 'blur'}
                      disabled={media.bgBusy}
                      onClick={() => {
                        setPicker(false)
                        void media.applyBackground(media.bgMode === 'blur' ? 'none' : 'blur')
                      }}
                    >
                      {t('room.preEntrada.fundoDesfocado')}
                    </button>
                    <button
                      type="button"
                      className={cx('rm-fxchip', media.bgMode === 'image' && 'is-on')}
                      aria-pressed={media.bgMode === 'image'}
                      aria-expanded={picker && media.bgMode === 'image'}
                      disabled={media.bgBusy}
                      onClick={() => {
                        if (media.bgMode === 'image') {
                          setPicker((v) => !v)
                          return
                        }
                        setPicker(true)
                        if (media.presets[0]) void media.applyBackground('image', media.presets[0].url)
                      }}
                    >
                      {t('room.preEntrada.fundoVirtual')}
                    </button>
                  </div>
                )}
              </div>
            </div>

            {prejoin.second ? (
              <div className="rm-prejoin__preview rm-prejoin__preview--second">
                <video ref={prejoin.attachSecond} autoPlay playsInline muted className="rm-prejoin__video is-plain" />
                <div className="rm-prejoin__tags">
                  <span className="rm-flag">{t('room.preEntrada.fonteN', { n: 2, tipo: segundaLabel })}</span>
                </div>
                <div className="rm-prejoin__foot">
                  {secondMeta && <span className="rm-flag rm-flag--meta">{metaLonga(secondMeta, t('room.preEntrada.fps'))}</span>}
                  <span className="dx-spacer" />
                  <span className="rm-flag rm-flag--meta">{t('room.preEntrada.entraComoApresentacao')}</span>
                </div>
              </div>
            ) : (
              media.hasLocalVideo && (
                // O lugar da fonte 2 existe mesmo vazio: diz o que se pode pôr lá e porque não está.
                <div className="rm-prejoin__preview rm-prejoin__preview--empty">
                  <div className="rm-prejoin__tags">
                    <span className="rm-flag">{t('room.preEntrada.fonte2')}</span>
                  </div>
                  <div className="rm-prejoin__placeholder">
                    <Icon name="screen" size={22} />
                    <span>
                      {info && info.topology !== 'sfu'
                        ? t('room.preEntrada.fonte2SoSfu')
                        : devices.cams.length > 1
                          ? t('room.preEntrada.fonte2Escolher')
                          : t('room.preEntrada.fonte2Ligar')}
                    </span>
                  </div>
                </div>
              )
            )}
          </div>

          {/* Antes de entrar só há microfone e câmara — a barra da sala nasce na sala. */}
          <div className="rm-prejoin__controls">
            <div className="rm-controls__group">
              <button
                type="button"
                className={cx('rm-ctrl', !media.micOn && 'is-off')}
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
                className={cx('rm-ctrl', !showVideo && 'is-off')}
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

          {picker && media.bgMode === 'image' && (
            <div className="rm-prejoin__bgs" role="radiogroup" aria-label={t('room.preEntrada.fundoVirtual')}>
              <button
                type="button"
                role="radio"
                aria-checked={false}
                className="rm-prejoin__bg"
                disabled={media.bgBusy}
                onClick={() => {
                  setPicker(false)
                  void media.applyBackground('none')
                }}
              >
                <Icon name="ban" />
                <span>{t('room.definicoes.semEfeito')}</span>
              </button>
              {media.presets.map((p) => (
                <button
                  key={p.url}
                  type="button"
                  role="radio"
                  aria-checked={media.bgImageUrl === p.url}
                  className={cx('rm-prejoin__bg', media.bgImageUrl === p.url && 'is-on')}
                  disabled={media.bgBusy}
                  onClick={() => void media.applyBackground('image', p.url)}
                >
                  <img src={p.url} alt="" />
                  <span>{p.name}</span>
                </button>
              ))}
              <button type="button" className="rm-prejoin__bg" disabled={media.bgBusy} onClick={() => uploadRef.current?.click()}>
                <Icon name="upload" />
                <span>{t('room.definicoes.carregarImagem')}</span>
              </button>
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
            </div>
          )}
          {media.bgBusy && <p className="dx-muted rm-prejoin__busy">{t('room.definicoes.aAplicar')}</p>}

          {status && <Alert tone={semMedia ? 'warning' : undefined}>{status}</Alert>}
        </main>

        <aside className="rm-prejoin__side">
          <section className="rm-prejoin__group" aria-labelledby="rm-pj-cams">
            <h3 id="rm-pj-cams" className="rm-prejoin__label">
              {t('room.preEntrada.camaras')}
            </h3>
            <div role="radiogroup" aria-labelledby="rm-pj-cams" className="rm-prejoin__list">
              {devices.cams.length === 0 && <p className="dx-muted rm-prejoin__empty">{t('room.preEntrada.nenhumaCamara')}</p>}
              {devices.cams.map((d, i) => {
                const principal = d.deviceId === media.camId
                const segunda = !!d.deviceId && d.deviceId === prejoin.second?.deviceId
                const label = d.label || t('room.preEntrada.camaraN', { n: i + 1 })
                const meta = principal
                  ? camMeta
                    ? t('room.preEntrada.fonteMeta', { n: 1, meta: metaCurta(camMeta) })
                    : t('room.preEntrada.emUso')
                  : segunda && secondMeta
                    ? t('room.preEntrada.fonteMeta', { n: 2, meta: metaCurta(secondMeta) })
                    : undefined
                return (
                  <DeviceOption
                    key={d.deviceId || i}
                    selected={principal || segunda}
                    label={label}
                    meta={meta}
                    onSelect={() => void prejoin.switchDevice('cam', d.deviceId)}
                    extra={
                      podeSegundaFonte && !principal && d.deviceId ? (
                        <button
                          type="button"
                          className={cx('rm-devopt__extra', segunda && 'is-on')}
                          aria-pressed={segunda}
                          aria-label={t('room.preEntrada.usarComoFonte2Nome', { nome: label })}
                          onClick={() => void prejoin.toggleSecondCam(d.deviceId)}
                        >
                          {t('room.preEntrada.fonte2')}
                        </button>
                      ) : undefined
                    }
                  />
                )
              })}
            </div>
            {devices.cams.length > 1 && info && info.topology !== 'sfu' && <p className="dx-muted rm-prejoin__hint">{t('room.preEntrada.fonte2SoSfu')}</p>}
          </section>

          <section className="rm-prejoin__group" aria-labelledby="rm-pj-mics">
            <h3 id="rm-pj-mics" className="rm-prejoin__label">
              {t('room.preEntrada.microfones')}
            </h3>
            <div role="radiogroup" aria-labelledby="rm-pj-mics" className="rm-prejoin__list">
              {devices.mics.length === 0 && <p className="dx-muted rm-prejoin__empty">{t('room.preEntrada.nenhumMicrofone')}</p>}
              {devices.mics.map((d, i) => {
                const principal = d.deviceId === media.micId
                const misturado = !!d.deviceId && d.deviceId === prejoin.mixDeviceId
                const label = d.label || t('room.preEntrada.microfoneN', { n: i + 1 })
                return (
                  <DeviceOption
                    key={d.deviceId || i}
                    selected={principal || misturado}
                    label={label}
                    meta={misturado ? t('room.preEntrada.misturado') : undefined}
                    onSelect={() => void prejoin.switchDevice('mic', d.deviceId)}
                    extra={
                      podeMisturar && !principal && !PSEUDO.has(d.deviceId) ? (
                        <button
                          type="button"
                          className={cx('rm-devopt__extra', misturado && 'is-on')}
                          aria-pressed={misturado}
                          aria-label={t('room.preEntrada.misturarNome', { nome: label })}
                          onClick={() => void prejoin.toggleSecondMic(d.deviceId)}
                        >
                          {t('room.preEntrada.misturar')}
                        </button>
                      ) : undefined
                    }
                  >
                    {principal && (
                      <MicLevel stream={prejoin.mixLevels?.[0] ?? prejoin.previewStream} version={prejoin.previewVersion} muted={!media.micOn} />
                    )}
                    {misturado && prejoin.mixLevels && (
                      <MicLevel stream={prejoin.mixLevels[1]} version={prejoin.previewVersion} muted={!media.micOn} />
                    )}
                  </DeviceOption>
                )
              })}
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
            <div className="rm-devrow">
              <select
                className="rm-devrow__select"
                aria-labelledby="rm-pj-out"
                value={media.speakerId}
                onChange={(e) => media.setSpeakerId(e.target.value)}
              >
                <option value="">{t('room.preEntrada.predefinidoSistema')}</option>
                {devices.speakers
                  .filter((d) => d.deviceId && d.deviceId !== 'default')
                  .map((d, i) => (
                    <option key={d.deviceId} value={d.deviceId}>
                      {d.label || t('room.preEntrada.altifalanteN', { n: i + 1 })}
                    </option>
                  ))}
              </select>
              <button type="button" className="rm-devrow__test" onClick={media.testSpeaker}>
                {t('room.preEntrada.testar')}
              </button>
            </div>
            <VolumeSlider media={media} id="rm-pj-vol" />
          </section>

          <div className="rm-prejoin__cta">
            <Button variant="primary" size="lg" block onClick={() => onJoin(false)}>
              {t('room.preEntrada.entrar')}
            </Button>
            {media.hasLocalVideo && (
              <button type="button" className="rm-prejoin__alt" onClick={() => onJoin(true)}>
                {t('room.preEntrada.entrarApenasAudio')}
              </button>
            )}
          </div>
        </aside>
      </div>
    </div>
  )
}

/** Volume do que se ouve NESTE dispositivo (pré-entrada e definições). */
export function VolumeSlider({ media, id }: { media: LocalMedia; id: string }) {
  const { t } = useTranslation()
  return (
    <div className="rm-volume">
      <label htmlFor={id} className="dx-num dx-muted">
        {t('room.preEntrada.vol')}
      </label>
      <input
        id={id}
        type="range"
        min={0}
        max={100}
        step={1}
        value={media.outputVolume}
        style={{ ['--v' as string]: `${media.outputVolume}%` }}
        aria-valuetext={t('room.preEntrada.volumeValor', { n: media.outputVolume })}
        title={t('room.preEntrada.volume')}
        onChange={(e) => media.setOutputVolume(Number(e.target.value))}
      />
      <span className="dx-num dx-muted rm-volume__n">{media.outputVolume}</span>
    </div>
  )
}
