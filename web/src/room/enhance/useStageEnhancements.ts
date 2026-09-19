import { useMemo } from 'react'
import type { LocalConditions } from '../../layerPolicy'
import { useImmersiveStage } from '../immersive/useImmersiveStage'
import type { Layout } from '../useLayout'
import type { LocalMedia } from '../useLocalMedia'
import type { RoomCore } from '../useRoomCore'
import { useBatteryLow } from './deviceEnv'
import { immersiveTarget, sameTarget, sharpenTarget } from './target'
import { useReceiveSharpen } from './useReceiveSharpen'
import { useSharpSend } from './useSharpSend'

/**
 * Nitidez de envio, realce na recepção e palco imersivo — o único ponto de
 * entrada na sala. `Room.tsx` chama isto uma vez e passa o resultado a
 * `StageEnhancementsLayer` (dentro do palco) e a `EnhancementSettings` (nas
 * definições). Nada daqui altera o que é gravado.
 */
export function useStageEnhancements(core: RoomCore, media: LocalMedia, layout: Layout, conditions: LocalConditions) {
  const batteryLow = useBatteryLow()
  const cond = useMemo(() => ({ ...conditions, batteryLow: conditions.batteryLow || batteryLow }), [conditions, batteryLow])
  const send = useSharpSend(core, cond)

  const input = {
    presentationPeerId: core.presentation?.peerId ?? null,
    viewMode: layout.effectiveViewMode,
    stagePeerId: layout.stagePeer?.peerId ?? null,
    stageOnSelf: layout.stageOnSelf,
    peers: core.peers,
  }
  const immTarget = immersiveTarget(input)
  const shTarget = sharpenTarget(input)

  const immersive = useImmersiveStage(immTarget, {
    cameraTrackRef: core.cameraTrackRef,
    headRef: core.headRef,
    camOn: media.camOn && media.hasLocalVideo,
    speaking: core.speaking,
    dataSaver: !!cond.dataSaver,
  })
  // Os dois efeitos não se empilham no mesmo vídeo: o palco imersivo já
  // redesenha o orador; o realce fica para quando ele não está a correr.
  const receive = useReceiveSharpen(shTarget, immersive.running && sameTarget(immTarget, shTarget))

  return { send, receive, immersive }
}

export type StageEnhancements = ReturnType<typeof useStageEnhancements>
