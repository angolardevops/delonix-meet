// MÓDULO DE APOIO do `bar-responsivo.mjs` — não é um teste.
//
// Monta a barra de controlos VERDADEIRA da sala (`room/ControlBar.tsx`), com os
// textos reais em português, dentro da mesma estrutura de página que a sala
// usa. Os dados são fictícios; a marcação e o CSS não. A versão anterior
// copiava a barra à mão para um HTML — e uma cópia prova a cópia, não o produto.
import { createRoot } from 'react-dom/client'
import '../src/i18n'
import { ControlBar } from '../src/room/ControlBar'
import type { LocalMedia } from '../src/room/useLocalMedia'
import type { Layout } from '../src/room/useLayout'

const nada = () => {}
const media = {
  micOn: true,
  camOn: true,
  hasLocalVideo: true,
  toggleMic: async () => {},
  toggleCam: async () => {},
  devices: { mics: [], cams: [], speakers: [] },
  micId: '',
  camId: '',
  speakerId: '',
  switchMic: async () => {},
  switchCam: async () => {},
  setSpeakerId: nada,
  noiseSuppression: true,
  toggleNoiseSuppression: async () => {},
  testSpeaker: nada,
  bgMode: 'none',
  bgBusy: false,
  applyBackground: async () => {},
  parallax: false,
  toggleParallax: async () => {},
} as unknown as LocalMedia
const layout = {
  effectiveViewMode: 'grid',
  setViewMode: nada,
  setPinnedId: nada,
  presLayout: 'side',
  setPresLayout: nada,
  fullscreen: false,
  toggleFullscreen: nada,
  hideSelf: false,
  setHideSelf: nada,
  hideNoVideo: false,
  setHideNoVideo: nada,
} as unknown as Layout

createRoot(document.getElementById('barra')!).render(
  <ControlBar
    isHost
    topology="sfu"
    status="Sem ligação ao servidor — tentativa 3 de 6…"
    callState="degraded"
    media={media}
    layout={layout}
    panel="none"
    onTogglePanel={nada}
    onOpenSettings={nada}
    presenterLabel="Teresa Kiala apresenta"
    returnTo="sala-principal"
    onReturnToMain={nada}
    breakoutEndsAt={Math.floor(Date.now() / 1000) + 600}
    timerEndsAt={Math.floor(Date.now() / 1000) + 300}
    ccOn={false}
    onToggleCc={nada}
    onReaction={nada}
    sharing={false}
    shareNeedsPermission={false}
    onShare={nada}
    handRaised={false}
    onToggleHand={nada}
    recording={false}
    recBusy={false}
    onToggleRecording={nada}
    wbOpen={false}
    onToggleWhiteboard={nada}
    transcribing
    unreadChat={12}
    total={14}
    openQuestions={6}
    openPolls={1}
    hasPresentation
    pipDisponivel
    pipOn={false}
    pipErro={null}
    onTogglePip={nada}
    multicamAvailable
    onOpenMulticam={nada}
    serverRecAvailable
    serverRecOn={false}
    onToggleServerRec={nada}
    onLeave={nada}
    fonte2Label="Elgato CamLink 4K"
    fonte2On={false}
    onFonte={nada}
    canAdmit
    waitingCount={2}
    onAdmitAll={nada}
  />,
)
