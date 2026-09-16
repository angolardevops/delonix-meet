import { CSSProperties, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'
import { ParticipantTile, SpeakingBars, TileAvatar, TileFrame } from './ParticipantTile'
import { PresentationTile } from './PresentationTile'
import type { LocalMedia } from './useLocalMedia'
import type { Layout } from './useLayout'
import type { QosReport } from '../webrtc'
import type { RemotePeer, RoomCore } from './useRoomCore'

export interface StageProps {
  core: RoomCore
  media: LocalMedia
  layout: Layout
  qos: QosReport | null
  handRaised: boolean
  onTilePin: (peerId: string) => void
  onTileMute: (peerId: string) => void
  onTileKick: (peerId: string) => void
  onRequestControl?: () => void
  children?: ReactNode
}

/** Palco: grelha, orador em destaque, ou apresentação com a plateia. */
export function Stage({ core, media, layout, qos, handRaised, onTilePin, onTileMute, onTileKick, onRequestControl, children }: StageProps) {
  const { t } = useTranslation()
  const { peers, speaking, presentation, isHost, sharing, topology } = core
  const me = currentUser()?.username ?? ''
  const meSpeaking = speaking.has('me') && media.micOn
  const { effectiveViewMode, visiblePeers, tileSize, pinnedId } = layout
  const isSolo = layout.total === 1
  const gridSized = !presentation && effectiveViewMode === 'grid' && !isSolo
  const w = gridSized ? tileSize.w : undefined
  const h = gridSized ? tileSize.h : undefined

  // Em mesh a partilhar, o próprio <video> mostra o ecrã (substitui a câmara);
  // no SFU o ecrã é outro retrato. Nunca um rectângulo preto: sem vídeo, avatar.
  const showSelfVideo = (media.hasLocalVideo && media.camOn) || (sharing && topology !== 'sfu')
  const selfTile = (
    <TileFrame kind="local" peerId="me" name={me} speaking={meSpeaking} pinned={pinnedId === 'me'} width={w} height={h} onDoubleClick={() => onTilePin('me')}>
      <video
        ref={media.attachLocalVideo}
        autoPlay
        muted
        playsInline
        className={cx(
          'rm-tile__video',
          media.hasLocalVideo && !sharing && media.bgMode === 'none' && 'is-mirror',
          !showSelfVideo && 'is-hidden',
        )}
      />
      {!showSelfVideo && <TileAvatar name={me} />}
      <div className="rm-tile__flags">
        {handRaised && (
          <span className="rm-flag rm-flag--live">
            <Icon name="hand" size={11} />
            {t('room.tile.mao')}
          </span>
        )}
        {sharing && (
          <span className="rm-flag">
            <Icon name="screen" size={11} />
            {t('room.tile.aPartilhar')}
          </span>
        )}
      </div>
      <div className="rm-tile__foot">
        <span className="rm-tile__name">
          {media.micOn ? (
            meSpeaking ? <SpeakingBars /> : <Icon name="mic" size={11} />
          ) : (
            <>
              <Icon name="micOff" size={11} className="dx-icon rm-tile__muted" />
              <span className="dx-sr-only">{t('room.tile.microfoneDesligado')}</span>
            </>
          )}
          <span className="rm-tile__label">{me ? t('room.tile.nomeTu', { nome: me }) : t('room.tile.tu')}</span>
        </span>
        {isHost && <span className="rm-tile__role">{t('room.papel.anfitriao')}</span>}
      </div>
      <div className="rm-tile__actions">
        <button
          type="button"
          className={cx('rm-tile__btn', pinnedId === 'me' && 'is-on')}
          onClick={() => onTilePin('me')}
          aria-pressed={pinnedId === 'me'}
          aria-label={pinnedId === 'me' ? t('room.tile.desafixarMe') : t('room.tile.fixarMe')}
          title={pinnedId === 'me' ? t('room.tile.desafixarMe') : t('room.tile.fixarMe')}
        >
          <Icon name="pin" size={13} />
        </button>
      </div>
    </TileFrame>
  )

  const remoteTile = (p: RemotePeer, big = false) => (
    <ParticipantTile
      key={p.peerId}
      peer={p}
      isHost={isHost}
      speaking={speaking.has(p.peerId)}
      pinned={pinnedId === p.peerId}
      weak={(qos?.byPeer[p.peerId]?.lossPct ?? 0) > 5}
      width={big ? undefined : w}
      height={big ? undefined : h}
      onPin={onTilePin}
      onMute={onTileMute}
      onKick={onTileKick}
    />
  )

  const parallax: CSSProperties = media.parallaxStyle
  let body: ReactNode

  if (presentation) {
    const presenter = presentation.peerId === 'me'
    const label = presenter
      ? t('room.apresentacao.estasAApresentar')
      : t('room.apresentacao.estaAApresentar', { nome: peers.find((p) => p.peerId === presentation.peerId)?.username ?? '' })
    // Quem apresenta não se vê na plateia: o ecrã já é o conteúdo.
    const audienceSelf = !layout.hideSelf && !presenter
    const count = visiblePeers.length + (audienceSelf ? 1 : 0)
    body = (
      <div className={cx('rm-stage rm-stage--pres', layout.presLayout === 'side' ? 'is-side' : 'is-bottom')} style={parallax}>
        <div className="rm-stage__main">
          <PresentationTile
            stream={presentation.stream}
            label={label}
            own={presenter}
            onRequestControl={onRequestControl}
            onToggleFullscreen={layout.toggleFullscreen}
          />
        </div>
        {count > 0 && (
          <div className="rm-strip" aria-label={t('room.palco.plateia')}>
            <span className="rm-strip__head dx-eyebrow">{t('room.palco.participantes', { count: peers.length + 1 })}</span>
            {audienceSelf && selfTile}
            {visiblePeers.map((p) => remoteTile(p))}
          </div>
        )}
      </div>
    )
  } else if (effectiveViewMode === 'stage' && (layout.stageOnSelf || layout.stagePeer)) {
    const onSelf = layout.stageOnSelf || !layout.stagePeer
    const audience = onSelf ? visiblePeers : visiblePeers.filter((p) => p.peerId !== layout.stagePeer!.peerId)
    const showAudienceSelf = !onSelf && layout.showSelf
    body = (
      <div className={cx('rm-stage rm-stage--speaker', layout.presLayout === 'side' ? 'is-side' : 'is-bottom')} style={parallax}>
        <div className="rm-stage__main">{onSelf ? selfTile : remoteTile(layout.stagePeer!, true)}</div>
        {(audience.length > 0 || showAudienceSelf) && (
          <div className="rm-strip" aria-label={t('room.palco.plateia')}>
            {showAudienceSelf && selfTile}
            {audience.map((p) => remoteTile(p))}
          </div>
        )}
      </div>
    )
  } else {
    body = (
      <>
        <div className={cx('rm-grid', isSolo && 'is-solo')} style={parallax}>
          {layout.showSelf && selfTile}
          {visiblePeers.map((p) => remoteTile(p))}
        </div>
        {layout.pageCount > 1 && (
          <nav className="rm-pager" aria-label={t('room.palco.paginas')}>
            <button
              type="button"
              className="dx-iconbtn"
              onClick={() => layout.setGridPage((pg) => Math.max(0, pg - 1))}
              disabled={layout.page === 0}
              aria-label={t('room.palco.paginaAnterior')}
            >
              <Icon name="chevronLeft" />
            </button>
            <span className="dx-num">
              {layout.page + 1} / {layout.pageCount}
            </span>
            <button
              type="button"
              className="dx-iconbtn"
              onClick={() => layout.setGridPage((pg) => Math.min(layout.pageCount - 1, pg + 1))}
              disabled={layout.page >= layout.pageCount - 1}
              aria-label={t('room.palco.paginaSeguinte')}
            >
              <Icon name="chevronRight" />
            </button>
            <span className="dx-muted">{t('room.palco.ouvesTodos', { count: layout.filteredPeers.length })}</span>
          </nav>
        )}
      </>
    )
  }

  return (
    <div className="rm-stagearea" ref={layout.areaRef}>
      {body}
      {children}
    </div>
  )
}
