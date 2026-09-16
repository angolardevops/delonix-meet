import { useEffect, useState } from 'react'
import type { RoomCore } from './useRoomCore'

export const REACTION_EMOJIS = ['👍', '❤️', '😂', '🎉', '👏', '😮']

export interface FloatingReaction {
  id: number
  emoji: string
  username: string
  own: boolean
}

let seq = 0

export function useReactions(core: RoomCore) {
  const { signal } = core
  const [reactions, setReactions] = useState<FloatingReaction[]>([])
  const [handRaised, setHandRaised] = useState(false)

  function float(emoji: string, username: string, own: boolean) {
    const id = ++seq
    setReactions((rs) => [...rs, { id, emoji, username, own }])
    setTimeout(() => setReactions((rs) => rs.filter((r) => r.id !== id)), 3500)
  }

  useEffect(() => signal.on('reaction', (m) => float(m.emoji, m.username, false)), [signal])

  function sendReaction(emoji: string) {
    signal.send({ type: 'reaction', emoji })
    float(emoji, '', true)
  }

  function toggleHand() {
    const raised = !handRaised
    setHandRaised(raised)
    signal.send({ type: 'hand', raised })
  }

  return { reactions, handRaised, sendReaction, toggleHand }
}
