import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { intlLocale } from '../i18n'
import { currentUser } from '../api'
import { Icon } from '../ui/icons'
import { usePresence } from '../components/PresenceProvider'
import { Button, Dialog, IconButton, Tabs } from '../ui/kit'
import '../ui/room.css'
import { AudioSink } from '../room/AudioSink'
import { ChatPanel } from '../room/ChatPanel'
import { ControlBar } from '../room/ControlBar'
import { InviteDialog } from '../room/InviteDialog'
import { MulticamPanel } from '../room/MulticamPanel'
import { Notices } from '../room/Notices'
import { NotesPanel } from '../room/NotesPanel'
import { PeoplePanel } from '../room/PeoplePanel'
import { PollsPanel } from '../room/PollsPanel'
import { Prejoin } from '../room/Prejoin'
import { QaPanel } from '../room/QaPanel'
import { SettingsPanel } from '../room/SettingsPanel'
import { Stage } from '../room/Stage'
import { CaptionOverlay, ReactionsLayer, ReadyCard, SpotlightQuestion, WaitingOverlay, WinnerOverlay } from '../room/StageOverlays'
import { EndedScreen, PassphraseScreen } from '../room/StateScreens'
import { TopBar } from '../room/TopBar'
import { VoiceCall } from '../room/VoiceCall'
import { Whiteboard } from '../room/Whiteboard'
import { useBreakouts } from '../room/useBreakouts'
import { entradaDirecta, useCallSession } from '../room/useCallSession'
import { useChat } from '../room/useChat'
import { useInvite } from '../room/useInvite'
import { useLayout } from '../room/useLayout'
import { destinosNoAr, useLive } from '../room/useLive'
import { useLocalMedia } from '../room/useLocalMedia'
import { useMeetingTools } from '../room/useMeetingTools'
import { useMulticam } from '../room/useMulticam'
import { useParticipants } from '../room/useParticipants'
import { usePip } from '../room/usePip'
import { usePrejoin } from '../room/usePrejoin'
import { useReactions } from '../room/useReactions'
import { useRecording } from '../room/useRecording'
import { useRemoteControl } from '../room/useRemoteControl'
import { useRoomChrome, type Panel } from '../room/useRoomChrome'
import { useRoomCore, type RemotePeer } from '../room/useRoomCore'
import { useScreenShare } from '../room/useScreenShare'
import { useTranscription } from '../room/useTranscription'
import { useWhiteboard } from '../room/useWhiteboard'
import { aEditar } from '../room/wbState'

type Confirmacao = { kind: 'transfer'; peer: RemotePeer } | { kind: 'server-rec-e2ee' } | null

/**
 * A sala. Toda a LÓGICA vive em hooks (`room/use*.ts`) — sessão, media,
 * participantes, ferramentas — e esta página só compõe a vista por cima deles.
 */
export default function Room({
  code,
  voiceOnly = false,
  onLeave,
  onSwitch,
}: {
  code: string
  voiceOnly?: boolean
  onLeave: () => void
  onSwitch?: (code: string) => void
}) {
  const { t, i18n } = useTranslation()
  const locale = intlLocale(i18n.language)
  const [initial] = useState(() => (entradaDirecta(code, voiceOnly) ? 'connecting' : 'prejoin') as 'connecting' | 'prejoin')

  const core = useRoomCore(code, initial)
  const media = useLocalMedia(core)
  const session = useCallSession(core, media, { voiceOnly, onLeave, onSwitch })
  const prejoin = usePrejoin(core, media, session.joinIntentRef)
  const inRoom = core.roomState === 'in'
  const chrome = useRoomChrome(code, core.peers.length, inRoom)
  const participants = useParticipants(core, chrome.panel === 'people')
  const layout = useLayout(core, participants.conditions)
  const pip = usePip(core, layout.pinnedId)
  const chat = useChat(core, chrome.panel === 'chat')
  const reactions = useReactions(core)
  const transcription = useTranscription(core)
  const recording = useRecording(core, {
    onServerStopped: transcription.saveOnServerRecordingStop,
    onUploaded: () => chrome.setPanel('people'),
  })
  const share = useScreenShare(core, media)
  const whiteboard = useWhiteboard(core)
  const tools = useMeetingTools(core)
  const breakouts = useBreakouts(core, onSwitch)
  const remote = useRemoteControl(core)
  const invite = useInvite(code)
  const multicam = useMulticam(core)
  const live = useLive(core)

  const [secOpen, setSecOpen] = useState(false)
  // «Serão admitidos ao entrar» (pré-entrada): quem admite e viu gente à
  // porta admite-a toda assim que entra (`admit-all`, validado no servidor).
  const admitirAoEntrar = useRef(false)
  const admitAll = participants.admitAll
  useEffect(() => {
    if (!inRoom || !admitirAoEntrar.current) return
    admitirAoEntrar.current = false
    admitAll()
  }, [inRoom, admitAll])
  /** O painel de sondagens abriu pelo atalho do chat: foca o compositor. */
  const [pollFromChat, setPollFromChat] = useState(false)
  const [confirmacao, setConfirmacao] = useState<Confirmacao>(null)
  /**
   * Vista da chamada. Uma chamada de VOZ abre no ecrã de voz (sem câmara nem
   * grelha); «passar a vídeo» ou «ver vídeo» mudam para a sala na MESMA
   * sessão — a ligação ao SFU não se refaz.
   */
  const [view, setView] = useState<'voice' | 'video'>(voiceOnly ? 'voice' : 'video')
  const presence = usePresence()
  const directCall = presence.directCall(code)

  const { peers, isHost } = core
  const { speakerId } = media
  const { companion } = session

  // Callbacks ESTÁVEIS para os retratos (achado 2.3): uma identidade só, e o
  // retrato devolve o `peerId` que recebeu.
  const togglePin = layout.togglePin
  const onTilePin = useCallback((peerId: string) => togglePin(peerId), [togglePin])
  const onTileMute = participants.mute
  const onTileKick = participants.kick

  // Atalhos Ctrl+D (microfone) e Ctrl+E (câmara). Chamam a acção DIRECTAMENTE:
  // procurar o botão pelo `aria-label` partia-se em inglês e em francês.
  // Na pré-entrada os mesmos atalhos actuam sobre a pré-visualização.
  const atalhosRef = useRef({ media, prejoin, prejoinAtivo: core.roomState === 'prejoin' })
  atalhosRef.current = { media, prejoin, prejoinAtivo: core.roomState === 'prejoin' }
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return
      if (!(e.ctrlKey || e.metaKey)) return
      const k = e.key.toLowerCase()
      if (k !== 'd' && k !== 'e') return
      e.preventDefault()
      const { media: m, prejoin: pj, prejoinAtivo } = atalhosRef.current
      if (prejoinAtivo) pj.toggle(k === 'd' ? 'mic' : 'cam')
      else if (k === 'd') void m.toggleMic()
      else void m.toggleCam()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  // A segunda fonte escolhida na pré-entrada entra como apresentação assim que
  // a media liga — pelo caminho da partilha, com as mesmas permissões.
  const shareSource = share.shareSource
  useEffect(() => {
    if (session.callState !== 'connected') return
    const fonte = core.secondSourceRef.current
    if (!fonte) return
    core.secondSourceRef.current = null
    void shareSource(fonte)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.callState])

  const talkOverNames = useMemo(() => {
    if (!participants.talkOver) return null
    const nomes = participants.talkOverIds
      .map((id) => (id === 'me' ? t('room.tile.tu') : peers.find((p) => p.peerId === id)?.username ?? ''))
      .filter(Boolean)
    return new Intl.ListFormat(i18n.language, { type: 'conjunction' }).format(nomes)
  }, [participants.talkOver, participants.talkOverIds, peers, t, i18n.language])

  function leave() {
    void session.leave(transcription.saveOnLeave)
  }

  /** Liga a câmara nesta sessão; só muda de vista se a câmara ligou mesmo. */
  async function goVideo() {
    if (!media.hasLocalVideo || !media.camOn) await media.toggleCam()
    if (core.localStreamRef.current?.getVideoTracks().length) setView('video')
  }


  function toggleServerRecording() {
    if (recording.serverRec) {
      recording.setServerRecording(false)
      return
    }
    // E2EE: gravar exige CEDER a chave ao servidor — pede-se consentimento.
    if (session.e2eeOn) {
      setConfirmacao({ kind: 'server-rec-e2ee' })
      return
    }
    recording.setServerRecording(true)
  }

  function openMulticam() {
    multicam.openPanel()
    chrome.setPanel('multicam')
  }

  // O foco pedido pelo atalho vale uma vez.
  useEffect(() => {
    if (chrome.panel !== 'polls') setPollFromChat(false)
  }, [chrome.panel])

  function closePanel() {
    if (chrome.panel === 'multicam') void multicam.close()
    chrome.closePanel()
  }

  // ── Ecrãs que substituem a sala ───────────────────────────────────────────
  let content
  if (core.roomState === 'prejoin') {
    content = (
      <Prejoin
        code={code}
        media={media}
        prejoin={prejoin}
        status={core.status}
        onCancel={onLeave}
        onJoin={(audioOnly) => {
          if (prejoin.waiting && prejoin.waiting.length > 0) admitirAoEntrar.current = true
          if (audioOnly) prejoin.dropVideo()
          session.join(audioOnly)
        }}
      />
    )
  } else if (core.roomState === 'e2ee-pass') {
    content = <PassphraseScreen code={code} onSubmit={session.submitPassphrase} onCancel={onLeave} />
  } else if (core.roomState === 'denied' || core.roomState === 'kicked' || core.roomState === 'notfound') {
    content = <EndedScreen kind={core.roomState} onLeave={onLeave} />
  } else if (view === 'voice') {
    content = (
      <div className="rm-shell">
        <VoiceCall
          roomState={core.roomState}
          callState={session.callState}
          status={core.status}
          peers={peers}
          speaking={core.speaking}
          media={media}
          call={directCall}
          roomName={session.roomName}
          companion={companion}
          onUseAudioHere={session.dismissCompanion}
          onGoVideo={goVideo}
          onViewVideo={() => setView('video')}
          onHangup={leave}
        />
        <AudioSink peers={peers} sinkId={speakerId} mudo={companion} volume={media.outputVolume} />
      </div>
    )
  } else {
    // Três separadores, como no template; as sondagens aparecem no fio do chat e
    // o compositor abre como painel próprio («Nova sondagem»).
    const tabbed = chrome.panel === 'chat' || chrome.panel === 'qa' || chrome.panel === 'people'
    const openQuestions = tools.questions.filter((q) => !q.answered).length
    const openPolls = tools.polls.filter((p) => p.open).length
    const presenterLabel = core.presentation
      ? t('room.topo.aPartilhar', {
          nome:
            core.presentation.peerId === 'me'
              ? currentUser()?.username ?? ''
              : peers.find((p) => p.peerId === core.presentation!.peerId)?.username ?? '',
        })
      : null
    const recordingLabel = recording.serverRec
      ? t('room.gravacao.noServidorPor', { nome: recording.serverRec.by })
      : recording.recording
        ? t('room.gravacao.estasAGravar')
        : recording.remoteRecorder
          ? t('room.gravacao.aGravarPor', { nome: recording.remoteRecorder })
          : null
    // Fonte 2: outra câmara deste dispositivo, publicada como apresentação. Só
    // em SFU (no mesh a partilha substitui a câmara), e não por cima de uma
    // partilha de ecrã que já esteja a decorrer.
    const outraCamara = media.devices.cams.find((d) => d.deviceId && d.deviceId !== media.camId)
    const fonte2 =
      core.topology === 'sfu' && outraCamara && (!share.sharing || share.sourceDeviceId)
        ? { deviceId: outraCamara.deviceId, label: outraCamara.label || t('room.controlos.fonte2') }
        : null
    const panelTitle: Partial<Record<Panel, string>> = {
      settings: t('room.painel.definicoes'),
      notes: t('room.painel.notas'),
      multicam: t('room.multicam.titulo'),
      polls: t('room.painel.sondagensTitulo'),
    }

    const editoresDoQuadro = aEditar(whiteboard.actividade, Date.now())
    const controlos = (
      <ControlBar
        isHost={isHost}
        topology={core.topology}
        status={core.status}
        callState={session.callState}
        media={media}
        layout={layout}
        panel={chrome.panel}
        onTogglePanel={chrome.togglePanel}
        onOpenSettings={() => chrome.setPanel('settings')}
        presenterLabel={presenterLabel}
        returnTo={breakouts.returnTo}
        onReturnToMain={breakouts.returnToMain}
        breakoutEndsAt={breakouts.endsAt}
        timerEndsAt={tools.timerEndsAt}
        ccOn={transcription.ccOn}
        onToggleCc={transcription.toggleCc}
        onReaction={reactions.sendReaction}
        sharing={share.sharing}
        shareNeedsPermission={share.needsPermission}
        onShare={share.requestOrToggleShare}
        handRaised={reactions.handRaised}
        onToggleHand={reactions.toggleHand}
        recording={recording.recording}
        recBusy={recording.recBusy}
        onToggleRecording={() => void recording.toggleLocal()}
        wbOpen={whiteboard.open}
        onToggleWhiteboard={whiteboard.toggle}
        transcribing={transcription.transcribing}
        unreadChat={chat.unread}
        total={peers.length + 1}
        openQuestions={openQuestions}
        openPolls={openPolls}
        hasPresentation={!!core.presentation}
        pipDisponivel={pip.pipDisponivel}
        pipOn={pip.pipOn}
        pipErro={pip.pipErro}
        onTogglePip={() => void pip.alternarPip()}
        multicamAvailable={isHost && multicam.supported}
        onOpenMulticam={openMulticam}
        serverRecAvailable={isHost && core.topology === 'sfu'}
        serverRecOn={!!recording.serverRec}
        onToggleServerRec={toggleServerRecording}
        onLeave={leave}
        fonte2Label={fonte2?.label ?? null}
        fonte2On={share.sharing && !!share.sourceDeviceId}
        onFonte={(f) => {
          const aPartilharFonte = share.sharing && !!share.sourceDeviceId
          if (f === 'fonte2' && !aPartilharFonte && fonte2) void share.shareSource(fonte2.deviceId)
          if (f === 'camara' && aPartilharFonte) share.requestOrToggleShare()
        }}
        canAdmit={session.canAdmit}
        waitingCount={participants.waitingQueue.length}
        onAdmitAll={participants.admitAll}
      />
    )

    content = (
      <div className="rm-shell">
        <TopBar
          title={session.roomName}
          code={code}
          joinedAt={core.startedAtRef.current || core.joinedAtRef.current}
          inRoom={inRoom}
          callState={session.callState}
          recordingLabel={recordingLabel}
          live={
            live.on
              ? { on: true, destinos: destinosNoAr(live), since: live.since }
              : { on: multicam.estado.fase === 'no-ar', destinos: [], since: null }
          }
          e2eeOn={session.e2eeOn}
          secOpen={secOpen}
          secCode={session.secCode}
          onToggleSec={() => setSecOpen((v) => !v)}
          waitingCount={session.canAdmit ? participants.waitingQueue.length : 0}
          onOpenPeople={() => chrome.togglePanel('people')}
          total={peers.length + 1}
          viewMode={layout.effectiveViewMode}
          onViewMode={(v) => {
            if (chrome.panel === 'multicam') closePanel()
            layout.setViewMode(v)
            if (v === 'grid') {
              layout.setPinnedId(null)
              layout.ignoreSpotlight()
            }
          }}
          studioAvailable={isHost && multicam.supported}
          studioOpen={chrome.panel === 'multicam'}
          onStudio={openMulticam}
          presenterLabel={presenterLabel}
          board={
            whiteboard.open
              ? {
                  sharedBy: whiteboard.openedBy,
                  saving: whiteboard.saving,
                  canSave: whiteboard.strokes.length > 0,
                  onSave: () => void whiteboard.save(),
                  pen: whiteboard.pen,
                  editors: editoresDoQuadro,
                  shared: whiteboard.pen.on || multicam.boardOnStage,
                }
              : null
          }
          locale={locale}
        />

        <div className="rm-body">
          <div className="rm-main">
            <Stage
              core={core}
              media={media}
              layout={layout}
              qos={participants.qos}
              handRaised={reactions.handRaised}
              onTilePin={onTilePin}
              onTileMute={onTileMute}
              onTileKick={onTileKick}
              onRequestControl={remote.requestControl}
              onOpenPeople={() => chrome.setPanel('people')}
            >
              <ReactionsLayer reactions={reactions.reactions} />
              {transcription.ccOn && transcription.caption && <CaptionOverlay caption={transcription.caption} />}
              {tools.winnerFx && <WinnerOverlay />}
              {tools.spotlitQuestion && <SpotlightQuestion q={tools.spotlitQuestion} />}
              {core.roomState === 'waiting' && <WaitingOverlay />}
            </Stage>

            {whiteboard.open && (
              <Whiteboard
                wb={whiteboard}
                me={currentUser()?.username ?? ''}
                myPeerId={core.meuPeerIdRef.current}
                isHost={isHost}
                controls={controlos}
                micOn={media.micOn}
                meSpeaking={core.speaking.has('me') && media.micOn}
                peers={peers}
                speaking={core.speaking}
                onOpenPeople={() => chrome.setPanel('people')}
                stage={
                  isHost && multicam.estado.fase === 'no-ar'
                    ? { destinos: multicam.destinos.filter((d) => d.chave.trim()).length, onStage: multicam.boardOnStage, setStream: multicam.setBoardStream }
                    : null
                }
              />
            )}

            <div className="rm-float">
              {chrome.readyOpen && isHost && inRoom && (
                <ReadyCard
                  code={code}
                  waitingRoomOn={session.waitingRoomOn}
                  onDismiss={chrome.dismissReady}
                  onAddPeople={() => {
                    chrome.dismissReady()
                    invite.show()
                  }}
                />
              )}
              {breakouts.announcement && (
                <section className="rm-notice" role="status" aria-label={t('room.paralelas.anuncio')}>
                  <header className="rm-notice__head">
                    <Icon name="bell" size={13} />
                    <strong>{t('room.paralelas.anuncioDe', { nome: breakouts.announcement.from })}</strong>
                    <span className="dx-spacer" />
                    <IconButton icon="x" bare label={t('room.avisos.dispensar')} onClick={breakouts.dismissAnnouncement} />
                  </header>
                  <p className="rm-notice__text">{breakouts.announcement.text}</p>
                </section>
              )}
              <Notices
                canAdmit={session.canAdmit}
                waitingQueue={participants.waitingQueue}
                onAdmit={participants.admit}
                onAdmitAll={participants.admitAll}
                ctrlAsk={remote.ctrlAsk}
                onAnswerCtrl={remote.answer}
                shareAsk={share.shareAsk}
                isHost={isHost}
                onAnswerShare={share.answerShareRequest}
                poll={tools.popup}
                myVote={tools.popup ? tools.myVotes[tools.popup.id] : undefined}
                onVote={tools.vote}
                onDismissPoll={tools.dismissPoll}
                companion={companion}
                onUseAudioHere={session.dismissCompanion}
                recNotice={recording.recNotice}
                talkOverNames={talkOverNames}
              />
            </div>
            {/* A barra vive na coluna principal: o painel lateral ocupa a altura toda
                (template DelonixRoomChat). Com o quadro aberto, vive por baixo da folha. */}
            {!whiteboard.open && controlos}
          </div>

          {chrome.panel !== 'none' && (
            <aside className="rm-panel" aria-label={tabbed ? t('room.painel.rotulo') : panelTitle[chrome.panel]}>
              <header className="rm-panel__head">
                {tabbed ? (
                  <Tabs<Panel>
                    label={t('room.painel.rotulo')}
                    value={chrome.panel}
                    onChange={chrome.setPanel}
                    tabs={[
                      { value: 'chat', label: t('room.painel.chat'), count: chat.unread },
                      { value: 'qa', label: t('room.painel.perguntas'), count: openQuestions },
                      { value: 'people', label: t('room.painel.participantes'), count: peers.length + 1 },
                    ]}
                  />
                ) : (
                  <h2 className="rm-panel__title">{panelTitle[chrome.panel]}</h2>
                )}
                <IconButton icon="x" bare label={t('room.painel.fechar')} onClick={closePanel} className={tabbed ? 'rm-panel__close is-tabbed' : 'rm-panel__close'} />
              </header>
              {chrome.panel === 'chat' && (
                <ChatPanel
                  chat={chat}
                  isHost={isHost}
                  code={code}
                  peers={peers}
                  tools={tools}
                  myRole={core.myRole}
                  onNewPoll={() => {
                    setPollFromChat(true)
                    chrome.setPanel('polls')
                  }}
                />
              )}
              {chrome.panel === 'qa' && <QaPanel tools={tools} isHost={isHost} />}
              {chrome.panel === 'polls' && <PollsPanel tools={tools} isHost={isHost} present={peers.length + 1} focusComposer={pollFromChat} />}
              {chrome.panel === 'people' && (
                <PeoplePanel
                  code={code}
                  isHost={isHost}
                  canAdmit={session.canAdmit}
                  isTraining={session.isTraining}
                  peers={peers}
                  speaking={core.speaking}
                  micOn={media.micOn}
                  qos={participants.qos}
                  participants={participants}
                  chatOn={chat.chatOn}
                  onChatOpenForAll={chat.setChatOpenForAll}
                  hostShareOnly={share.hostShareOnly}
                  onHostShareOnly={share.setHostShareOnly}
                  sharePerms={share.sharePerms}
                  onGrantShare={share.grantShare}
                  onTransferHost={(peer) => setConfirmacao({ kind: 'transfer', peer })}
                  breakouts={breakouts}
                  recordings={recording.recordings}
                  onDownload={recording.download}
                  onInvite={invite.show}
                  onPrivateMessage={(peer) => {
                    chat.setTarget({ peerId: peer.peerId, username: peer.username })
                    chrome.setPanel('chat')
                  }}
                  spotlightId={layout.spotlightId}
                  onSpotlight={layout.setSpotlight}
                />
              )}
              {chrome.panel === 'settings' && (
                <SettingsPanel media={media} transcription={transcription} localVideo={core.localVideoRef.current} />
              )}
              {chrome.panel === 'notes' && <NotesPanel transcription={transcription} isHost={isHost} />}
              {chrome.panel === 'multicam' && <MulticamPanel multicam={multicam} peers={peers} roomTitle={session.roomName || code} />}
            </aside>
          )}
        </div>

        {/* O áudio de TODOS, fora do palco: o que se ouve não depende do layout. */}
        <AudioSink peers={peers} sinkId={speakerId} mudo={companion} volume={media.outputVolume} />

        {invite.open && <InviteDialog invite={invite} />}
      </div>
    )
  }

  return (
    <div className="dx-stage rm-room" data-state={core.roomState}>
      {/* Vídeo da janela flutuante. Montado em TODOS os estados, para o ouvinte
          de `leavepictureinpicture` o encontrar logo ao montar. Não é
          `display: none` de propósito: um vídeo assim não tem imagem e o browser
          recusa a janela. */}
      <video
        ref={pip.pipVideo}
        muted
        autoPlay
        playsInline
        aria-hidden="true"
        tabIndex={-1}
        style={{ position: 'fixed', width: 1, height: 1, opacity: 0, pointerEvents: 'none', bottom: 0, left: 0 }}
      />
      {content}

      {confirmacao?.kind === 'transfer' && (
        <Dialog
          title={t('room.confirmar.passarAnfitriaoTitulo')}
          onClose={() => setConfirmacao(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirmacao(null)}>
                {t('room.confirmar.cancelar')}
              </Button>
              <Button
                variant="primary"
                icon="key"
                onClick={() => {
                  participants.transferHost(confirmacao.peer.peerId)
                  setConfirmacao(null)
                }}
              >
                {t('room.confirmar.passarAnfitriao')}
              </Button>
            </>
          }
        >
          {/* Passar o bastão é irreversível pelo próprio: só o novo anfitrião o devolve. */}
          <p>{t('room.confirmar.passarAnfitriaoTexto', { nome: confirmacao.peer.username })}</p>
        </Dialog>
      )}

      {confirmacao?.kind === 'server-rec-e2ee' && (
        <Dialog
          title={t('room.confirmar.gravarE2eeTitulo')}
          onClose={() => setConfirmacao(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirmacao(null)}>
                {t('room.confirmar.cancelar')}
              </Button>
              <Button
                variant="danger"
                icon="record"
                onClick={() => {
                  setConfirmacao(null)
                  const key = core.e2eeKeyRef.current
                  if (key) recording.setServerRecording(true, key)
                }}
              >
                {t('room.confirmar.gravarE2ee')}
              </Button>
            </>
          }
        >
          <p>{t('room.confirmar.gravarE2eeTexto')}</p>
          <p className="dx-muted">{t('room.confirmar.gravarE2eeNota', { nome: currentUser()?.username ?? '' })}</p>
        </Dialog>
      )}
    </div>
  )
}
