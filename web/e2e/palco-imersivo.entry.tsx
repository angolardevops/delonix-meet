// MÓDULO DE APOIO do `palco-imersivo.mjs` — não é um teste.
//
// Monta o realce e o palco imersivo VERDADEIROS (hooks `useReceiveSharpen` e
// `useImmersiveStage`, componentes `StageEnhancementsLayer` e
// `EnhancementSettings`) sobre a marcação do palco da sala, com um vídeo que é a
// câmara falsa do Chromium no lugar do vídeo recebido.
//
// Porque existe, além do `nitidez-imersivo.mjs` contra o servidor: o custo de
// um filtro por frame mede-se sobre um vídeo que CHEGA. Medido a 2026-09-16, a
// ligação ao SFU em 8190 caía para `disconnected` segundos depois de entrar — na
// branch base também, sem este código — e o vídeo remoto parava. Um banco que
// depende da rede para medir a GPU mede a rede.
import { useMemo, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import '../src/i18n'
import '../src/ui/tokens.css'
import '../src/ui/base.css'
import '../src/ui/room.css'
import { EnhancementSettings, StageEnhancementsLayer } from '../src/room/enhance/EnhancementViews'
import { useReceiveSharpen } from '../src/room/enhance/useReceiveSharpen'
import { useSharpSend } from '../src/room/enhance/useSharpSend'
import type { EnhanceTarget } from '../src/room/enhance/target'
import { useImmersiveStage } from '../src/room/immersive/useImmersiveStage'
import type { RoomCore } from '../src/room/useRoomCore'

const params = new URLSearchParams(location.search)
const W = Number(params.get('w') ?? 1280)
const H = Number(params.get('h') ?? 720)

function Banco() {
  const target: EnhanceTarget = { kind: 'peer', peerId: 'p1', name: 'Formadora' }
  const core = useMemo(
    () =>
      ({
        roomState: 'in',
        sharing: false,
        topology: 'sfu',
        callRef: { current: null },
        cameraTrackRef: { current: null },
        headRef: { current: null },
      }) as unknown as RoomCore,
    [],
  )
  const [speaking, setSpeaking] = useState(false)
  const speakingSet = useMemo(() => new Set(speaking ? ['p1'] : []), [speaking])
  const send = useSharpSend(core, {})
  const immersive = useImmersiveStage(target, { cameraTrackRef: core.cameraTrackRef, headRef: core.headRef, camOn: false, speaking: speakingSet, dataSaver: false })
  const receive = useReceiveSharpen(target, immersive.running)
  const enh = { send, receive, immersive }
  const videoRef = useRef<HTMLVideoElement | null>(null)

  return (
    <div className="dx-stage rm-room" style={{ display: 'flex', height: '100vh', minHeight: '100dvh' }}>
      <div className="rm-stagearea" style={{ flex: 1 }}>
        <div className="rm-stage rm-stage--speaker is-side">
          <div className="rm-stage__main">
            <div className="rm-tile" data-peer="remoto" data-peer-id="p1">
              <video
                ref={(el) => {
                  videoRef.current = el
                  if (!el || el.srcObject) return
                  void navigator.mediaDevices.getUserMedia({ video: { width: W, height: H } }).then((s) => {
                    el.srcObject = s
                    void el.play()
                  })
                }}
                autoPlay
                muted
                playsInline
                className="rm-tile__video"
              />
            </div>
          </div>
        </div>
        <StageEnhancementsLayer enh={enh} suggestImmersive roomCode="banco" />
      </div>
      <aside className="rm-panel" style={{ width: 340 }}>
        <div className="rm-scroll">
          <label>
            <input type="checkbox" data-banco="a-falar" checked={speaking} onChange={(e) => setSpeaking(e.target.checked)} /> a falar
          </label>
          <EnhancementSettings enh={enh} viewMode="stage" onSpeakerView={() => {}} />
        </div>
      </aside>
    </div>
  )
}

createRoot(document.getElementById('raiz')!).render(<Banco />)
