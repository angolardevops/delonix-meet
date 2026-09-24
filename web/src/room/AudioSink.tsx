import { useEffect, useRef } from 'react'
import type { RemotePeer } from './useRoomCore'

/**
 * O ÁUDIO de todos os participantes, num sítio só, SEMPRE montado.
 *
 * O que está no ecrã é decisão de layout; o que se ouve não pode depender do
 * layout. Com o `<audio>` dentro do retrato, esconder retratos (palco,
 * paginação, «esconder quem não tem vídeo») deixava a pessoa surda sem pista.
 *
 * `mudo` (companion, R114) silencia sem desmontar: o elemento continua ligado
 * ao stream, e ligar o som aqui é instantâneo.
 */
export function AudioSink({
  peers,
  sinkId,
  mudo,
  volume = 100,
}: {
  peers: RemotePeer[]
  sinkId: string
  mudo: boolean
  /** 0–100: volume escolhido neste dispositivo (pré-entrada ou definições). */
  volume?: number
}) {
  return (
    <div className="rm-audio-sink" aria-hidden="true" hidden>
      {peers.map((p) => (
        <PeerAudio key={p.peerId} stream={p.stream} sinkId={sinkId} mudo={mudo} volume={volume} />
      ))}
    </div>
  )
}

function PeerAudio({ stream, sinkId, mudo, volume }: { stream: MediaStream | null; sinkId: string; mudo: boolean; volume: number }) {
  const ref = useRef<HTMLAudioElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el || el.srcObject === stream) return
    el.srcObject = stream
    void el.play().catch(() => {})
  }, [stream])
  // Saída escolhida (altifalantes).
  useEffect(() => {
    const el = ref.current as (HTMLAudioElement & { setSinkId?: (id: string) => Promise<void> }) | null
    if (el?.setSinkId) void el.setSinkId(sinkId || '').catch(() => {})
  }, [sinkId, stream])
  useEffect(() => {
    if (ref.current) ref.current.volume = Math.max(0, Math.min(1, volume / 100))
  }, [volume, stream])
  return <audio ref={ref} autoPlay muted={mudo} />
}
