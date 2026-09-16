import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser, roomChatHistory } from '../api'
import { comConfirmada, comEnviada, comHistorico, comReaccoes, comRecebida, type ChatMsg, type HistoricoChat } from './chatState'
import type { RoomCore } from './useRoomCore'

export type { ChatMsg } from './chatState'

/** Emojis para inserir numa mensagem (diferentes das reacções flutuantes). */
export const CHAT_EMOJIS = [
  '😀', '😂', '😍', '🥰', '😎', '🤔', '🙏', '👍', '👎', '❤️',
  '🔥', '🎉', '✅', '⚠️', '📌', '💡', '🚀', '💪', '👏', '😭',
]

/** Reacções rápidas numa mensagem (as do template). */
export const CHAT_REACTIONS = ['👍', '🎯', '❤️', '😂', '👏']

export function useChat(core: RoomCore, chatOpen: boolean) {
  const { t } = useTranslation()
  const { signal, code, setStatus } = core
  const [messages, setMessages] = useState<ChatMsg[]>([])
  const [unread, setUnread] = useState(0)
  /** Regra do anfitrião (R92). Vem SEMPRE do servidor. */
  const [chatOn, setChatOn] = useState(true)
  const [input, setInput] = useState('')
  const [mentionQuery, setMentionQuery] = useState<string | null>(null)
  /** Mensagem a que se está a responder. */
  const [replyTo, setReplyTo] = useState<ChatMsg | null>(null)
  /** O servidor só dá contagens: o que EU reagi lembra-se aqui (alterna por conta). */
  const [mine, setMine] = useState<Record<string, string[]>>({})
  const openRef = useRef(chatOpen)
  openRef.current = chatOpen

  useEffect(() => {
    if (chatOpen) setUnread(0)
  }, [chatOpen])

  useEffect(() => {
    const offs = [
      signal.onB1('chat', (m) => {
        setMessages((c) => comRecebida(c, m))
        if (!openRef.current) setUnread((n) => n + 1)
      }),
      signal.onB1('chat-sent', (m) => setMessages((c) => comConfirmada(c, m))),
      signal.onB1('chat-reactions', (m) => setMessages((c) => comReaccoes(c, m))),
      // Histórico ao entrar (as ÚLTIMAS 200, com fios e reacções): melhor
      // esforço, não bloqueia a sala.
      signal.on('joined', () => {
        void roomChatHistory(code)
          .then((history) => setMessages((live) => comHistorico(live, history as HistoricoChat[], currentUser()?.id)))
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
    const clientId = `c${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`
    const mae = replyTo?.id ?? null
    signal.sendB1({ type: 'chat', text, reply_to: mae, client_id: clientId })
    setMessages((c) => comEnviada(c, { clientId, username: currentUser()?.username ?? '', text, replyTo: mae, at: Date.now() }))
    setInput('')
    setMentionQuery(null)
    setReplyTo(null)
  }

  /** Alterna uma reacção (o servidor devolve as contagens a toda a sala). */
  function react(id: string, emoji: string) {
    if (!chatOn && !core.isHost) {
      setStatus(t('room.estado.chatFechado'))
      return
    }
    signal.sendB1({ type: 'chat-react', id, emoji })
    setMine((m) => {
      const l = m[id] ?? []
      return { ...m, [id]: l.includes(emoji) ? l.filter((e) => e !== emoji) : [...l, emoji] }
    })
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
    replyTo,
    /** Só mensagens confirmadas (com id) aceitam respostas. */
    startReply: (m: ChatMsg | null) => setReplyTo(m && m.id ? m : null),
    react,
    myReactions: mine,
    setChatOpenForAll: (on: boolean) => signal.send({ type: 'chat-toggle', on }),
  }
}

export type Chat = ReturnType<typeof useChat>
