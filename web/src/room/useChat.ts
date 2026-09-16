import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser, roomChatHistory } from '../api'
import type { RoomCore } from './useRoomCore'

export interface ChatMsg {
  id: number
  username: string
  text: string
  own: boolean
  historical?: boolean
  /**
   * Quando chegou (ms). O histórico traz `created_at` do servidor; uma mensagem
   * ao vivo NÃO traz hora (`ServerMsg::Chat`), por isso é a hora de chegada a
   * este dispositivo — que numa sala ao vivo difere de milissegundos.
   */
  at: number
  /** peer_id de quem enviou (ao vivo) — para o papel na mensagem. */
  from?: string
}

/** Emojis para inserir numa mensagem (diferentes das reacções flutuantes). */
export const CHAT_EMOJIS = [
  '😀', '😂', '😍', '🥰', '😎', '🤔', '🙏', '👍', '👎', '❤️',
  '🔥', '🎉', '✅', '⚠️', '📌', '💡', '🚀', '💪', '👏', '😭',
]

let seq = 0

export function useChat(core: RoomCore, chatOpen: boolean) {
  const { t } = useTranslation()
  const { signal, code, setStatus } = core
  const [messages, setMessages] = useState<ChatMsg[]>([])
  const [unread, setUnread] = useState(0)
  /** Regra do anfitrião (R92). Vem SEMPRE do servidor. */
  const [chatOn, setChatOn] = useState(true)
  const [input, setInput] = useState('')
  const [mentionQuery, setMentionQuery] = useState<string | null>(null)
  const openRef = useRef(chatOpen)
  openRef.current = chatOpen

  useEffect(() => {
    if (chatOpen) setUnread(0)
  }, [chatOpen])

  useEffect(() => {
    const offs = [
      signal.on('chat', (m) => {
        setMessages((c) => [...c, { id: ++seq, username: m.username, text: m.text, own: false, at: Date.now(), from: m.from }])
        if (!openRef.current) setUnread((n) => n + 1)
      }),
      // Histórico ao entrar: melhor esforço, não bloqueia a sala.
      signal.on('joined', () => {
        void roomChatHistory(code)
          .then((history) =>
            setMessages((live) => [
              ...history.map((h) => ({
                id: ++seq,
                username: h.username,
                text: h.message,
                own: h.user_id === currentUser()?.id,
                historical: true,
                at: Date.parse(h.created_at) || Date.now(),
              })),
              ...live.filter((m) => !m.historical),
            ]),
          )
          .catch(() => {})
      }),
      signal.on('room-settings', (m) => setChatOn(m.chat_on ?? true)),
    ]
    return () => offs.forEach((off) => off())
  }, [signal, code])

  function changeInput(val: string) {
    setInput(val)
    const at = val.lastIndexOf('@')
    if (at !== -1 && !val.slice(at + 1).includes(' ')) setMentionQuery(val.slice(at + 1).toLowerCase())
    else setMentionQuery(null)
  }

  const mentionSuggestions =
    mentionQuery !== null
      ? core.peers
          .map((p) => p.username)
          .filter((n) => n.toLowerCase().startsWith(mentionQuery))
          .slice(0, 5)
      : []

  function completeMention(name: string) {
    const at = input.lastIndexOf('@')
    if (at === -1) return
    setInput(`${input.slice(0, at)}@${name} `)
    setMentionQuery(null)
  }

  function send() {
    const text = input.trim()
    if (!text) return
    // Com o chat fechado o servidor recusa (R92); o eco local faria a pessoa
    // acreditar que enviou. Recusar aqui é sobre DIZER-LHE.
    if (!chatOn && !core.isHost) {
      setStatus(t('room.estado.chatFechado'))
      return
    }
    signal.send({ type: 'chat', text })
    setMessages((c) => [...c, { id: ++seq, username: currentUser()?.username ?? '', text, own: true, at: Date.now() }])
    setInput('')
    setMentionQuery(null)
  }

  return {
    messages,
    unread,
    chatOn,
    input,
    setInput: changeInput,
    appendToInput: (s: string) => setInput((v) => v + s),
    mentionSuggestions,
    clearMention: () => setMentionQuery(null),
    completeMention,
    send,
    setChatOpenForAll: (on: boolean) => signal.send({ type: 'chat-toggle', on }),
  }
}

export type Chat = ReturnType<typeof useChat>
