import { CSSProperties, memo, ReactNode, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { avatarTone, cx, initials } from '../ui/kit'
import type { Role } from '../signaling'
import type { RemotePeer } from './useRoomCore'

/** Rótulo do papel num retrato; `null` para quem assiste. */
export function rotuloPapel(t: (k: string) => string, role: Role): string | null {
  switch (role) {
    case 'host':
      return t('room.papel.anfitriao')
    case 'cohost':
      return t('room.papel.coAnfitriao')
    case 'speaker':
      return t('room.papel.orador')
    case 'broadcast':
      return t('room.papel.emissao')
    default:
      return null
  }
}

/**
 * Chip do nome (template DelonixRoomGrid): «⊘» sem som, «◉» a falar, e o nome.
 * Com som e calado não há marca — o silêncio não precisa de ícone.
 */
export function TileName({ name, muted, speaking }: { name: string; muted: boolean; speaking: boolean }) {
  const { t } = useTranslation()
  return (
    <span className="rm-tile__name">
      {muted ? (
        <>
          <Icon name="ban" size={9} className="dx-icon rm-tile__muted" />
          <span className="dx-sr-only">{t('room.tile.microfoneDesligado')}</span>
        </>
      ) : (
        speaking && (
          <>
            <span className="rm-speakdot" aria-hidden="true" />
            <span className="dx-sr-only">{t('room.tile.aFalar')}</span>
          </>
        )
      )}
      <span className="rm-tile__label">{name}</span>
    </span>
  )
}

export function SpeakingBars() {
  return (
    <span className="rm-bars" aria-hidden="true">
      <i />
      <i />
      <i />
    </span>
  )
}

/** O invólucro comum ao retrato local e aos remotos. */
export function TileFrame({
  kind,
  peerId,
  name,
  speaking,
  pinned,
  reconnecting,
  width,
  height,
  onDoubleClick,
  children,
}: {
  kind: 'local' | 'remoto'
  peerId: string
  name: string
  speaking: boolean
  pinned: boolean
  reconnecting?: boolean
  width?: number
  height?: number
  onDoubleClick: () => void
  children: ReactNode
}) {
  const { t } = useTranslation()
  const style = { '--tone': avatarTone(name), width, height } as CSSProperties
  return (
    <div
      // Identidade estável para quem lê de fora — testes e leitores de ecrã.
      // Um atributo não muda quando a decoração muda; o texto sim (R90).
      className={cx('rm-tile', speaking && 'is-speaking', pinned && 'is-pinned', reconnecting && 'is-reconnecting')}
      data-peer={kind}
      data-peer-id={peerId}
      style={style}
      onDoubleClick={onDoubleClick}
      title={t('room.tile.duploCliqueFixa')}
    >
      {children}
    </div>
  )
}

export function TileAvatar({ name }: { name: string }) {
  return (
    <div className="rm-tile__avatar" aria-hidden="true">
      <span>{initials(name)}</span>
    </div>
  )
}

/**
 * Retrato de um participante remoto — MEMOIZADO (achados 2.2 e 2.3). Recebe o
 * `peerId` e devolve-o nos callbacks, para que um único callback estável sirva
 * todos os retratos; as dimensões chegam como números, não como objecto.
 */
export function ParticipantTileBase({
  peer,
  isHost,
  speaking,
  pinned,
  weak,
  width,
  height,
  onPin,
  onMute,
  onKick,
}: {
  peer: RemotePeer
  isHost: boolean
  speaking: boolean
  pinned: boolean
  /** Perda medida acima do limiar (só com amostras de QoS). */
  weak: boolean
  width?: number
  height?: number
  onPin: (peerId: string) => void
  onMute: (peerId: string) => void
  onKick: (peerId: string) => void
}) {
  const { t } = useTranslation()
  const ref = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    const el = ref.current
    if (el && el.srcObject !== peer.stream) {
      el.srcObject = peer.stream
      void el.play().catch(() => {})
    }
  }, [peer.stream])
  // Vídeo só com track E câmara ligada: uma track desactivada chega como preto.
  const hasVideo = !!peer.stream?.getVideoTracks().length && peer.camOn
  const hasAudio = !!peer.stream?.getAudioTracks().length && peer.micOn
  const role = rotuloPapel(t, peer.host ? 'host' : peer.role)
  return (
    <TileFrame
      kind="remoto"
      peerId={peer.peerId}
      name={peer.username}
      speaking={speaking && hasAudio}
      pinned={pinned}
      reconnecting={peer.reconnecting}
      width={width}
      height={height}
      onDoubleClick={() => onPin(peer.peerId)}
    >
      {/* O <video> nunca desmonta: esconde-se, para não perder o srcObject. */}
      <video ref={ref} autoPlay playsInline muted className={cx('rm-tile__video', !hasVideo && 'is-hidden')} />
      {!hasVideo && <TileAvatar name={peer.username} />}
      <div className="rm-tile__flags">
        {peer.hand && (
          <span className="rm-flag rm-flag--live">
            <Icon name="hand" size={10} />
            {t('room.tile.mao')}
          </span>
        )}
        {weak && (
          <span className="rm-flag rm-flag--warn">
            <Icon name="alert" size={10} />
            {t('room.tile.ligacaoFraca')}
          </span>
        )}
        {peer.is_pstn && (
          <span className="rm-flag rm-flag--plain">
            <Icon name="phone" size={10} />
            {t('room.papel.telefone')}
          </span>
        )}
        {peer.reconnecting && <span className="rm-flag">{t('room.tile.aVoltar')}</span>}
      </div>
      <div className="rm-tile__foot">
        <TileName name={peer.username} muted={!hasAudio} speaking={speaking && hasAudio} />
        {role && (
          <span className="rm-tile__role" title={peer.role === 'cohost' ? t('room.papel.coAnfitriaoDica') : undefined}>
            {role}
          </span>
        )}
        {peer.is_bot && (
          <span className="rm-tile__role">
            <Icon name="bot" size={10} />
            {t('room.papel.assistente')}
          </span>
        )}
      </div>
      <div className="rm-tile__actions">
        <button
          type="button"
          className={cx('rm-tile__btn', pinned && 'is-on')}
          onClick={() => onPin(peer.peerId)}
          aria-pressed={pinned}
          aria-label={pinned ? t('room.tile.desafixar', { nome: peer.username }) : t('room.tile.fixar', { nome: peer.username })}
          title={pinned ? t('room.tile.desafixar', { nome: peer.username }) : t('room.tile.fixar', { nome: peer.username })}
        >
          <Icon name="pin" size={13} />
        </button>
        {isHost && !peer.host && (
          <>
            <button
              type="button"
              className="rm-tile__btn"
              onClick={() => onMute(peer.peerId)}
              aria-label={t('room.tile.silenciar', { nome: peer.username })}
              title={t('room.tile.silenciar', { nome: peer.username })}
            >
              <Icon name="micOff" size={13} />
            </button>
            <button
              type="button"
              className="rm-tile__btn"
              onClick={() => onKick(peer.peerId)}
              aria-label={t('room.tile.remover', { nome: peer.username })}
              title={t('room.tile.remover', { nome: peer.username })}
            >
              <Icon name="x" size={13} />
            </button>
          </>
        )}
      </div>
    </TileFrame>
  )
}

/**
 * Comparação explícita: `peer` é um objecto novo a cada actualização de lista
 * mesmo quando nada mudou. Compara-se o que o retrato DESENHA — nem mais
 * (redesenha à toa) nem menos (fica preso).
 */
export const ParticipantTile = memo(ParticipantTileBase, (a, b) =>
  a.peer.peerId === b.peer.peerId &&
  a.peer.stream === b.peer.stream &&
  a.peer.username === b.peer.username &&
  a.peer.camOn === b.peer.camOn &&
  a.peer.micOn === b.peer.micOn &&
  a.peer.hand === b.peer.hand &&
  a.peer.host === b.peer.host &&
  a.peer.canAdmit === b.peer.canAdmit &&
  a.peer.role === b.peer.role &&
  a.peer.reconnecting === b.peer.reconnecting &&
  a.peer.is_pstn === b.peer.is_pstn &&
  a.peer.is_bot === b.peer.is_bot &&
  a.isHost === b.isHost &&
  a.speaking === b.speaking &&
  a.pinned === b.pinned &&
  a.weak === b.weak &&
  a.width === b.width &&
  a.height === b.height &&
  a.onPin === b.onPin &&
  a.onMute === b.onMute &&
  a.onKick === b.onKick,
)
