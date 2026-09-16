import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
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
import { CaptionOverlay, ReactionsLayer, ReadyCard, WaitingOverlay, WinnerOverlay } from '../room/StageOverlays'
import { EndedScreen, PassphraseScreen } from '../room/StateScreens'
import { TopBar } from '../room/TopBar'
import { Whiteboard } from '../room/Whiteboard'
import { useBreakouts } from '../room/useBreakouts'
import { entradaDirecta, useCallSession } from '../room/useCallSession'
import { useChat } from '../room/useChat'
import { useInvite } from '../room/useInvite'
import { useLayout } from '../room/useLayout'
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
  const locale = i18n.language === 'en' ? 'en-GB' : i18n.language === 'fr' ? 'fr-FR' : 'pt-PT'
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

  const [secOpen, setSecOpen] = useState(false)
  const [confirmacao, setConfirmacao] = useState<Confirmacao>(null)

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
  const mediaRef = useRef(media)
  mediaRef.current = media
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return
      if (!(e.ctrlKey || e.metaKey)) return
      const k = e.key.toLowerCase()
      if (k === 'd') {
        e.preventDefault()
        void mediaRef.current.toggleMic()
      } else if (k === 'e') {
        e.preventDefault()
        void mediaRef.current.toggleCam()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

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
          if (audioOnly) prejoin.dropVideo()
          session.join(audioOnly)
        }}
      />
    )
  } else if (core.roomState === 'e2ee-pass') {
    content = <PassphraseScreen code={code} onSubmit={session.submitPassphrase} onCancel={onLeave} />
  } else if (core.roomState === 'denied' || core.roomState === 'kicked') {
    content = <EndedScreen kind={core.roomState} onLeave={onLeave} />
  } else {
    const tabbed = chrome.panel === 'chat' || chrome.panel === 'qa' || chrome.panel === 'polls' || chrome.panel === 'people'
    const openQuestions = tools.questions.filter((q) => !q.answered).length
    const openPolls = tools.polls.filter((p) => p.open).length
    const presenterLabel = core.presentation
      ? core.presentation.peerId === 'me'
        ? t('room.apresentacao.aApresentar')
        : t('room.apresentacao.apresenta', { nome: peers.find((p) => p.peerId === core.presentation!.peerId)?.username ?? '' })
      : null
    const recordingLabel = recording.serverRec
      ? t('room.gravacao.noServidorPor', { nome: recording.serverRec.by })
      : recording.recording
        ? t('room.gravacao.estasAGravar')
        : recording.remoteRecorder
          ? t('room.gravacao.aGravarPor', { nome: recording.remoteRecorder })
          : null
    const panelTitle: Partial<Record<Panel, string>> = {
      settings: t('room.painel.definicoes'),
      notes: t('room.painel.notas'),
      multicam: t('room.multicam.titulo'),
    }

    content = (
      <div className="rm-shell">
        <TopBar
          title={session.roomName}
          code={code}
          joinedAt={core.joinedAtRef.current}
          inRoom={inRoom}
          callState={session.callState}
          recordingLabel={recordingLabel}
          live={multicam.estado.fase === 'no-ar'}
          e2eeOn={session.e2eeOn}
          secOpen={secOpen}
          secCode={session.secCode}
          onToggleSec={() => setSecOpen((v) => !v)}
          isInstant={session.isInstant}
          isTraining={session.isTraining}
          waitingCount={session.canAdmit ? participants.waitingQueue.length : 0}
          onOpenPeople={() => chrome.togglePanel('people')}
          total={peers.length + 1}
          viewMode={layout.effectiveViewMode}
          onViewMode={(v) => {
            layout.setViewMode(v)
            if (v === 'grid') layout.setPinnedId(null)
          }}
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
            >
              <ReactionsLayer reactions={reactions.reactions} />
              {transcription.ccOn && transcription.caption && <CaptionOverlay caption={transcription.caption} />}
              {tools.winnerFx && <WinnerOverlay />}
              {core.roomState === 'waiting' && <WaitingOverlay />}
            </Stage>

            {whiteboard.open && (
              <Whiteboard
                strokes={whiteboard.strokes}
                onStroke={whiteboard.addStroke}
                onClear={whiteboard.clear}
                onSave={whiteboard.save}
                onClose={whiteboard.close}
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
                      { value: 'polls', label: t('room.painel.sondagens'), count: openPolls },
                      { value: 'people', label: t('room.painel.participantes'), count: peers.length + 1 },
                    ]}
                  />
                ) : (
                  <h2 className="rm-panel__title">{panelTitle[chrome.panel]}</h2>
                )}
                <IconButton icon="x" bare label={t('room.painel.fechar')} onClick={closePanel} />
              </header>
              {chrome.panel === 'chat' && <ChatPanel chat={chat} isHost={isHost} />}
              {chrome.panel === 'qa' && <QaPanel tools={tools} isHost={isHost} />}
              {chrome.panel === 'polls' && <PollsPanel tools={tools} isHost={isHost} />}
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
                />
              )}
              {chrome.panel === 'settings' && (
                <SettingsPanel media={media} transcription={transcription} localVideo={core.localVideoRef.current} />
              )}
              {chrome.panel === 'notes' && <NotesPanel transcription={transcription} isHost={isHost} />}
              {chrome.panel === 'multicam' && <MulticamPanel multicam={multicam} peers={peers} />}
            </aside>
          )}
        </div>

        {/* O áudio de TODOS, fora do palco: o que se ouve não depende do layout. */}
        <AudioSink peers={peers} sinkId={speakerId} mudo={companion} />

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
        />

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
