import { CSSProperties, memo, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { CloseIcon, HandIcon, MicOffIcon, PinIcon } from '../icons'

/**
 * O mosaico de um participante — extraído de `Room.tsx` e MEMOIZADO
 * (achados 2.2 e 2.3 do docs/ux-perf-review.md).
 *
 * PORQUÊ: `memo(` aparecia ZERO vezes num ficheiro de 4 254 linhas com três
 * `setInterval` de 1 Hz a chamar `setState` na raiz. Cada tique reconciliava
 * todos os mosaicos e os seus elementos `<video>` para actualizar um contador
 * de segundos. Os relógios saíram para `room/Clocks.tsx`; isto trata da outra
 * metade — quando a raiz TEM mesmo de voltar a renderizar (alguém entra, o
 * chat mexe, o painel abre), os mosaicos que não mudaram ficam quietos.
 *
 * O `memo` só vale se as props forem estáveis: por isso o mosaico recebe
 * `peerId` e devolve-o nos callbacks, em vez de receber uma closure nova por
 * render (2.3). O `style` chega desmontado em `w`/`h` pela mesma razão.
 */

export interface RemotePeer {
  peerId: string
  username: string
  host: boolean
  hand: boolean
  camOn: boolean
  micOn: boolean
  /** Foi promovido a co-admitir entradas (o anfitrião tem-no sempre). */
  canAdmit: boolean
  stream: MediaStream | null
  is_pstn?: boolean
  is_bot?: boolean
  /** O socket caiu e o lugar está reservado (R91). O retrato fica no sítio,
   *  esbatido, em vez de desaparecer — uma quebra de rede deixa de parecer
   *  que a pessoa saiu e voltou a entrar. */
  reconnecting?: boolean
}

export function peerColor(name: string): string {
  let h = 5381
  for (let i = 0; i < name.length; i++) h = ((h << 5) + h + name.charCodeAt(i)) | 0
  // Duotone (estilo template): dois tons do mesmo matiz — mantém contraste com texto branco.
  const hue = Math.abs(h) % 360
  return `linear-gradient(135deg, hsl(${hue}, 45%, 27%), hsl(${(hue + 35) % 360}, 50%, 16%))`
}

export function SpeakingBars() {
  return (
    <span className="speaking-bars" aria-hidden>
      <i /><i /><i />
    </span>
  )
}

/** Exportado SÓ para o banco de ensaio poder medir com e sem `memo`. */
export function RemoteTileBase({
  peer,
  isHost,
  speaking,
  style,
  pinned,
  onPin,
  onMute,
  onKick,
}: {
  peer: RemotePeer
  isHost: boolean
  speaking: boolean
  style?: CSSProperties
  pinned?: boolean
  onPin?: (peerId: string) => void
  onMute: (peerId: string) => void
  onKick: (peerId: string) => void
}) {
  const { t } = useTranslation()
  const ref = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    if (ref.current && ref.current.srcObject !== peer.stream) {
      ref.current.srcObject = peer.stream
      void ref.current.play().catch(() => {})
    }
  }, [peer.stream])
  // Vídeo só quando há track E o peer diz que a câmara está ligada —
  // track com enabled=false chega como frames pretos, não como ausência.
  const hasVideo = !!peer.stream?.getVideoTracks().length && peer.camOn
  const hasAudio = !!peer.stream?.getAudioTracks().length && peer.micOn
  return (
    <div
      className={[
        'tile',
        speaking ? 'speaking' : '',
        peer.reconnecting ? 'reconnecting' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      // Identidade estável do retrato, para quem o lê de fora: testes de
      // interface e leitores de ecrã. Antes um teste distinguia o retrato local
      // do remoto por o TEXTO conter «eu» — e isso partiu-se quando os glifos
      // decorativos passaram a SVG (R88), porque um `<svg>` não tem
      // `textContent`. Uma asserção sobre texto decorativo quebra-se sempre que
      // a decoração muda; um atributo não.
      data-peer="remoto"
      data-peer-id={peer.peerId}
      style={style}
      onDoubleClick={() => onPin?.(peer.peerId)}
      title={t('room.espera.duploCliqueParaFixar')}
    >
      <video ref={ref} autoPlay playsInline muted style={{ display: hasVideo ? undefined : 'none' }} />
      {!hasVideo && (
        <div className="tile-avatar" style={{ background: peerColor(peer.username) }}>
          <span className="avatar-circle" style={{ background: 'rgba(0,0,0,0.25)' }}>{peer.username.slice(0, 2).toUpperCase()}</span>
        </div>
      )}
      <button
        className={pinned ? 'tile-pin pinned' : 'tile-pin'}
        onClick={() => onPin?.(peer.peerId)}
        title={pinned ? 'Desafixar do palco' : 'Fixar no palco'}
      >
        <PinIcon />
      </button>
      {peer.hand && <span className="hand-badge"><HandIcon /></span>}
      {/* Indicador de mic muted no canto superior direito (estilo Meet). */}
      {!hasAudio && (
        <span className="tile-mic-status" aria-label="microfone desativado">
          <MicOffIcon />
        </span>
      )}
      {peer.reconnecting && <span className="tile-reconnecting">a voltar…</span>}
      <span className="tile-name">
        {hasAudio && speaking && <SpeakingBars />}
        {peer.username}
        {peer.host ? ' · anfitrião' : ''}
        {peer.is_pstn ? ' · 📞 PSTN' : peer.is_bot ? ' · 🤖 AI Bot' : ''}
      </span>
      {isHost && !peer.host && (
        <div className="host-actions">
          <button title={t('lobby.mute')} onClick={() => onMute(peer.peerId)}>
            <MicOffIcon />
          </button>
          <button title={t('lobby.remove')} onClick={() => onKick(peer.peerId)}>
            <CloseIcon />
          </button>
        </div>
      )}
    </div>
  )
}

/**
 * Comparação explícita: o `memo` por omissão faz igualdade rasa, e `peer` é um
 * objecto novo a cada actualização de lista mesmo quando nada mudou. Compara-se
 * o que o mosaico DESENHA — nem mais (redesenha à toa), nem menos (fica preso).
 */
export const RemoteTile = memo(RemoteTileBase, (a, b) =>
  a.peer.peerId === b.peer.peerId &&
  a.peer.stream === b.peer.stream &&
  a.peer.username === b.peer.username &&
  a.peer.camOn === b.peer.camOn &&
  a.peer.micOn === b.peer.micOn &&
  a.peer.hand === b.peer.hand &&
  a.peer.host === b.peer.host &&
  a.peer.is_pstn === b.peer.is_pstn &&
  a.peer.is_bot === b.peer.is_bot &&
  a.isHost === b.isHost &&
  a.speaking === b.speaking &&
  a.pinned === b.pinned &&
  a.style?.width === b.style?.width &&
  a.style?.height === b.style?.height &&
  a.onPin === b.onPin && a.onMute === b.onMute && a.onKick === b.onKick,
)
