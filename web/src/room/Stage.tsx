import { CSSProperties, ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'
import { ParticipantTile, rotuloPapel, SpeakingBars, TileAvatar, TileFrame, TileName } from './ParticipantTile'
import { PresentationTile } from './PresentationTile'
import { ligacaoFraca } from './qosAmostra'
import { repartirFila } from './stripCapacity'
import type { LocalMedia } from './useLocalMedia'
import type { Layout } from './useLayout'
import { useStripCapacity } from './useStripCapacity'
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
  /** O «+N pessoas» da fila abre a lista completa. */
  onOpenPeople: () => void
  children?: ReactNode
}

/**
 * A câmara de quem apresenta, por cima do conteúdo partilhado (canto inferior
 * direito). Minimiza-se: tapar um diapositivo não é uma escolha nossa.
 */
function PresenterCam({ stream, name, camOn, speaking, mirror }: { stream: MediaStream | null; name: string; camOn: boolean; speaking: boolean; mirror: boolean }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLVideoElement>(null)
  const [min, setMin] = useState(false)
  const hasVideo = camOn && !!stream?.getVideoTracks().length
  useEffect(() => {
    const el = ref.current
    if (el && el.srcObject !== stream) {
      el.srcObject = stream
      void el.play().catch(() => {})
    }
  }, [stream, min, hasVideo])
  if (min)
    return (
      <button type="button" className="rm-prescam rm-prescam--min" onClick={() => setMin(false)} aria-label={t('room.apresentacao.mostrarCamara', { nome: name })} title={t('room.apresentacao.mostrarCamara', { nome: name })}>
        <Icon name="video" size={13} />
      </button>
    )
  return (
    <div className={cx('rm-prescam', speaking && 'is-speaking')} style={{ ['--tone' as string]: 'var(--accent-strong)' }}>
      <video ref={ref} autoPlay playsInline muted className={cx('rm-tile__video', !hasVideo && 'is-hidden', mirror && 'is-mirror')} />
      {!hasVideo && <TileAvatar name={name} />}
      <span className="rm-prescam__name">
        {speaking && <SpeakingBars />}
        {name}
      </span>
      <button type="button" className="rm-prescam__hide" onClick={() => setMin(true)} aria-label={t('room.apresentacao.esconderCamara')} title={t('room.apresentacao.esconderCamara')}>
        <Icon name="minus" size={11} />
      </button>
    </div>
  )
}

/** Palco: grelha, orador em destaque, ou apresentação com a plateia. */
export function Stage({ core, media, layout, qos, handRaised, onTilePin, onTileMute, onTileKick, onRequestControl, onOpenPeople, children }: StageProps) {
  const { t } = useTranslation()
  const { peers, speaking, presentation, isHost, sharing, topology } = core
  const me = currentUser()?.username ?? ''
  const meSpeaking = speaking.has('me') && media.micOn
  const { effectiveViewMode, visiblePeers, tileSize, pinnedId } = layout
  const isSolo = layout.total === 1
  // Na grelha os retratos enchem as células (CSS grid); o tamanho só se passa
  // fora dela, onde a fila decide.
  const w = undefined
  const h = undefined
  const strip = useStripCapacity('.rm-strip > .rm-tile, .rm-strip > .rm-strip__more')
  // A minha câmara por cima do que apresento: com fundo, a saída do efeito.
  const fxOut = media.bgMode !== 'none' ? core.effectRef.current?.output ?? null : null
  const fxStream = useMemo(() => (fxOut ? new MediaStream([fxOut]) : null), [fxOut])

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
            <Icon name="hand" size={10} />
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
        <TileName name={me ? t('room.tile.nomeTu', { nome: me }) : t('room.tile.tu')} muted={!media.micOn} speaking={meSpeaking} />
        {rotuloPapel(t, core.myRole) && <span className="rm-tile__role">{rotuloPapel(t, core.myRole)}</span>}
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
      weak={ligacaoFraca(qos?.byPeer[p.peerId]?.lossPct)}
      width={big ? undefined : w}
      height={big ? undefined : h}
      onPin={onTilePin}
      onMute={onTileMute}
      onKick={onTileKick}
    />
  )

  /** Plateia com «+N»: os que cabem, e um botão para a lista completa. */
  const fila = (incluirMe: boolean, lista: RemotePeer[], cabecalho?: ReactNode) => {
    const total = lista.length + (incluirMe ? 1 : 0)
    const { mostrar, resto } = repartirFila(total, strip.capacity)
    const remotos = lista.slice(0, Math.max(0, mostrar - (incluirMe ? 1 : 0)))
    return (
      <div className="rm-strip" ref={strip.ref} aria-label={t('room.palco.plateia')}>
        {cabecalho}
        {incluirMe && mostrar > 0 && selfTile}
        {remotos.map((p) => remoteTile(p))}
        {resto > 0 && (
          <button type="button" className="rm-strip__more" onClick={onOpenPeople} aria-label={t('room.palco.maisPessoasDica', { count: resto })}>
            {t('room.palco.maisPessoas', { count: resto })}
          </button>
        )}
      </div>
    )
  }

  const parallax: CSSProperties = media.parallaxStyle
  let body: ReactNode

  if (presentation) {
    const presenter = presentation.peerId === 'me'
    const presenterPeer = presenter ? null : peers.find((p) => p.peerId === presentation.peerId) ?? null
    const label = presenter
      ? t('room.apresentacao.estasAApresentar')
      : t('room.apresentacao.estaAApresentar', { nome: presenterPeer?.username ?? '' })
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
          {presenter ? (
            <PresenterCam
              stream={fxStream ?? core.localStreamRef.current}
              name={me}
              camOn={media.hasLocalVideo && media.camOn}
              speaking={meSpeaking}
              mirror={media.bgMode === 'none'}
            />
          ) : (
            presenterPeer && (
              <PresenterCam
                stream={presenterPeer.stream}
                name={presenterPeer.username}
                camOn={presenterPeer.camOn}
                speaking={speaking.has(presenterPeer.peerId)}
                mirror={false}
              />
            )
          )}
        </div>
        {count > 0 &&
          fila(
            audienceSelf,
            visiblePeers,
            <span className="rm-strip__head dx-eyebrow" data-strip-head>
              {t('room.palco.participantes', { count: peers.length + 1 })}
            </span>,
          )}
      </div>
    )
  } else if (effectiveViewMode === 'stage' && (layout.stageOnSelf || layout.stagePeer)) {
    const onSelf = layout.stageOnSelf || !layout.stagePeer
    const audience = onSelf ? visiblePeers : visiblePeers.filter((p) => p.peerId !== layout.stagePeer!.peerId)
    const showAudienceSelf = !onSelf && layout.showSelf
    const aFalar = onSelf ? meSpeaking : speaking.has(layout.stagePeer!.peerId)
    body = (
      <div className={cx('rm-stage rm-stage--speaker', layout.presLayout === 'side' ? 'is-side' : 'is-bottom')} style={parallax}>
        <div className="rm-stage__main">
          {onSelf ? selfTile : remoteTile(layout.stagePeer!, true)}
          {aFalar && (
            <span className="rm-flag rm-stage__talking" role="status">
              <SpeakingBars />
              {t('room.palco.aFalar')}
            </span>
          )}
        </div>
        {(audience.length > 0 || showAudienceSelf) && fila(showAudienceSelf, audience)}
      </div>
    )
  } else {
    body = (
      <>
        <div
          className={cx('rm-grid', isSolo && 'is-solo')}
          style={{ ...parallax, gridTemplateColumns: `repeat(${tileSize.cols}, minmax(0, 1fr))`, gridTemplateRows: `repeat(${tileSize.rows}, minmax(0, 1fr))` }}
        >
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
