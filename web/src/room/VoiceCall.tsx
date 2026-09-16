import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { AvatarStack, Button, Select, Toggle, avatarTone, cx, initials } from '../ui/kit'
import '../ui/voicecall.css'
import { MeetingElapsed } from './Clocks'
import { Ctrl } from './ControlBar'
import { PopoverPanel, usePopover } from './Popover'
import type { LocalMedia } from './useLocalMedia'
import type { RemotePeer } from './useRoomCore'
import { DirectCall, voicePhase } from './voiceCall'

/**
 * O ecrã de uma CHAMADA DE VOZ (DelonixCall / DelonixMobile): avatar grande,
 * quem é, o estado e o cronómetro, e quatro controlos — som, dispositivo de
 * áudio, passar a vídeo, desligar. Sem grelha de retratos e sem câmara.
 *
 * A chamada continua a ser uma sala (a mesma sessão, o mesmo SFU): só a vista
 * e a captura mudam. «Passar a vídeo» liga a câmara NESTA sessão — o
 * `enableVideo` do SFU acrescenta a track e renegoceia — e mostra a sala.
 */
export function VoiceCall({
  roomState,
  callState,
  status,
  peers,
  speaking,
  media,
  call,
  roomName,
  companion,
  onUseAudioHere,
  onGoVideo,
  onViewVideo,
  onHangup,
}: {
  roomState: string
  callState: string
  status: string
  peers: RemotePeer[]
  speaking: Set<string>
  media: LocalMedia
  call: DirectCall | null
  roomName: string
  companion: boolean
  onUseAudioHere: () => void
  /** Liga a minha câmara e passa à vista de vídeo. */
  onGoVideo: () => Promise<void>
  /** Só muda a vista (alguém do outro lado ligou a câmara). */
  onViewVideo: () => void
  /** Sair da sala. Se ninguém atendeu, o `PresenceProvider` envia `call-cancel`
   *  ao sair (o toque do outro lado pára e fica a chamada perdida). */
  onHangup: () => void
}) {
  const { t } = useTranslation()
  const audioPop = usePopover()
  const [since, setSince] = useState(0)
  const [goingVideo, setGoingVideo] = useState(false)
  const everHadPeer = since > 0

  // O cronómetro conta desde que há ALGUÉM do outro lado — não desde que eu
  // entrei na sala, que é quando comecei a chamar.
  useEffect(() => {
    if (peers.length > 0 && !since) setSince(Date.now())
  }, [peers.length, since])

  const phase = voicePhase({ roomState, callState, peers: peers.length, everHadPeer, call })
  const name =
    peers.length === 1 ? peers[0].username : peers.length > 1 ? t('room.voz.pessoas', { count: peers.length + 1 }) : call?.peer_name || roomName
  const group = peers.length > 1
  const remoteSpeaking = peers.some((p) => speaking.has(p.peerId))
  const withCamera = peers.find((p) => p.camOn && !!p.stream?.getVideoTracks().length)

  const phaseText: Record<typeof phase, string> = {
    connecting: t('room.voz.fase.aLigar'),
    calling: t('room.voz.fase.aChamar'),
    declined: t('room.voz.fase.recusou'),
    unavailable: t('room.voz.fase.indisponivel'),
    waiting: t('room.voz.fase.aEspera'),
    'in-call': t('room.voz.fase.emChamada'),
    reconnecting: t('room.voz.fase.aRestabelecer'),
    ended: t('room.voz.fase.terminou'),
  }

  async function goVideo() {
    setGoingVideo(true)
    try {
      await onGoVideo()
    } finally {
      setGoingVideo(false)
    }
  }

  return (
    <div className="vc" data-testid="voice-call" data-phase={phase}>
      <header className="vc-top">
        <span className="vc-kind">
          <Icon name="phone" size={12} />
          {t('room.voz.chamadaDeVoz')}
        </span>
      </header>

      <main className="vc-center">
        <div
          className={cx('vc-face', remoteSpeaking && 'is-speaking', (phase === 'calling' || phase === 'connecting') && 'is-ringing')}
          style={{ background: avatarTone(name) }}
          aria-hidden="true"
        >
          {group ? <Icon name="people" size={44} /> : initials(name)}
        </div>
        <h1 className="vc-name">{name}</h1>
        {group && <AvatarStack names={peers.map((p) => p.username)} max={5} size={26} />}
        <p className="vc-state" role="status" aria-live="polite">
          <span data-testid="voice-phase">{phaseText[phase]}</span>
          {phase === 'in-call' && since > 0 && (
            <>
              <span aria-hidden="true"> · </span>
              <MeetingElapsed startedAt={since} className="dx-num" />
            </>
          )}
        </p>
        {phase === 'unavailable' && <p className="vc-note">{t('room.voz.perdidaNota')}</p>}
        {withCamera && (
          <button type="button" className="vc-chip" onClick={onViewVideo}>
            <Icon name="video" size={12} />
            {t('room.voz.ligouCamara', { nome: withCamera.username })}
            <strong>{t('room.voz.verVideo')}</strong>
          </button>
        )}
        {companion && (
          <div className="vc-note vc-note--warn">
            <span>{t('room.voz.companion')}</span>
            <Button size="sm" variant="outline" onClick={onUseAudioHere}>
              {t('room.voz.usarAudioAqui')}
            </Button>
          </div>
        )}
        {status && <p className="vc-note">{status}</p>}
      </main>

      <footer className="vc-controls" role="group" aria-label={t('room.voz.controlos')}>
        <Ctrl
          icon={media.micOn ? 'mic' : 'micOff'}
          label={media.micOn ? t('room.controlos.desligarMicrofone') : t('room.controlos.ligarMicrofone')}
          caption={t('room.controlos.rotuloSom')}
          off={!media.micOn}
          pressed={!media.micOn}
          onClick={() => void media.toggleMic()}
        />
        <div className="rm-split vc-split" ref={audioPop.wrapRef}>
          <Ctrl
            icon="volume"
            label={t('room.controlos.opcoesAudio')}
            caption={t('room.voz.audio')}
            active={audioPop.open}
            popup
            expanded={audioPop.open}
            onClick={audioPop.toggle}
          />
          {audioPop.open && (
            <PopoverPanel label={t('room.controlos.opcoesAudio')} className="rm-devpop">
              <label className="rm-devpop__row">
                <span>{t('room.definicoes.microfone')}</span>
                <Select value={media.micId} onChange={(e) => void media.switchMic(e.target.value)}>
                  {media.devices.mics.length === 0 && <option value="">{t('room.definicoes.semDispositivos')}</option>}
                  {media.devices.mics.map((d, i) => (
                    <option key={d.deviceId || i} value={d.deviceId}>
                      {d.label || t('room.preEntrada.microfoneN', { n: i + 1 })}
                    </option>
                  ))}
                </Select>
              </label>
              <label className="rm-devpop__row">
                <span>{t('room.definicoes.altifalantes')}</span>
                <Select value={media.speakerId} onChange={(e) => media.setSpeakerId(e.target.value)}>
                  <option value="">{t('room.preEntrada.predefinidoSistema')}</option>
                  {media.devices.speakers
                    .filter((d) => d.deviceId && d.deviceId !== 'default')
                    .map((d, i) => (
                      <option key={d.deviceId} value={d.deviceId}>
                        {d.label || t('room.preEntrada.altifalanteN', { n: i + 1 })}
                      </option>
                    ))}
                </Select>
              </label>
              <Toggle label={t('room.definicoes.supressaoRuido')} checked={media.noiseSuppression} onChange={() => void media.toggleNoiseSuppression()} />
              <div className="rm-devpop__actions">
                <Button size="sm" variant="outline" icon="volume" onClick={media.testSpeaker}>
                  {t('room.preEntrada.testarSom')}
                </Button>
              </div>
            </PopoverPanel>
          )}
        </div>
        <Ctrl
          icon="video"
          label={t('room.voz.passarAVideo')}
          caption={t('room.controlos.rotuloVideo')}
          onClick={() => void goVideo()}
          active={goingVideo}
        />
        <Ctrl
          icon="phoneOff"
          label={t('room.voz.desligar')}
          caption={t('room.voz.desligarCurto')}
          danger
          className="rm-ctrl--hangup"
          onClick={onHangup}
        />
      </footer>
    </div>
  )
}
