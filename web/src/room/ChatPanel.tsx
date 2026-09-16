import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Avatar, IconButton, cx } from '../ui/kit'
import { chatEmTexto, nomeFicheiroChat } from './chatExport'
import { PollCard } from './PollCard'
import { CHAT_EMOJIS, type Chat, type ChatMsg } from './useChat'
import type { MeetingTools } from './useMeetingTools'
import type { RemotePeer } from './useRoomCore'

/** Markdown mínimo em linha: **negrito**, *itálico*, `código`, @menção. */
function ChatText({ text }: { text: string }) {
  const parts = text.split(/(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`|@\w+)/g)
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith('**') && part.endsWith('**') && part.length > 4) return <strong key={i}>{part.slice(2, -2)}</strong>
        if (part.startsWith('*') && part.endsWith('*') && part.length > 2) return <em key={i}>{part.slice(1, -1)}</em>
        if (part.startsWith('`') && part.endsWith('`') && part.length > 2) return <code key={i}>{part.slice(1, -1)}</code>
        if (part.startsWith('@')) return <span key={i} className="rm-chat__mention">{part}</span>
        return <Fragment key={i}>{part}</Fragment>
      })}
    </>
  )
}

type Item = { kind: 'msg'; at: number; msg: ChatMsg; index: number } | { kind: 'poll'; at: number; id: string }

export function ChatPanel({
  chat,
  isHost,
  code,
  peers,
  tools,
  onNewPoll,
}: {
  chat: Chat
  isHost: boolean
  code: string
  peers: RemotePeer[]
  tools: MeetingTools
  /** Atalho do anfitrião para o compositor de sondagens. */
  onNewPoll: () => void
}) {
  const { t, i18n } = useTranslation()
  const locale = i18n.language === 'en' ? 'en-GB' : i18n.language === 'fr' ? 'fr-FR' : 'pt-PT'
  const [emojiOpen, setEmojiOpen] = useState(false)
  const listRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const blocked = !chat.chatOn && !isHost
  const presentes = peers.length + 1

  // Abrir o chat é para escrever: o foco vai para o campo (sem isto, o que se
  // escrevia logo a seguir perdia-se).
  useEffect(() => {
    if (!blocked) inputRef.current?.focus()
  }, [blocked])

  // O fio: mensagens e sondagens pela ordem em que chegaram a este dispositivo.
  const itens = useMemo<Item[]>(() => {
    const out: Item[] = chat.messages.map((msg, index) => ({ kind: 'msg', at: msg.at, msg, index }))
    for (const p of tools.polls) {
      const at = tools.pollSeenAt[p.id]
      if (at != null) out.push({ kind: 'poll', at, id: p.id })
    }
    // Estável: empates mantêm a ordem de chegada das mensagens.
    return out.sort((a, b) => a.at - b.at || (a.kind === 'msg' && b.kind === 'msg' ? a.index - b.index : 0))
  }, [chat.messages, tools.polls, tools.pollSeenAt])

  // Segue a conversa: o item novo fica à vista.
  useEffect(() => {
    const el = listRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [itens.length])

  /** Papel de quem escreveu, lido de quem está na sala AGORA. */
  function papel(m: ChatMsg): string | null {
    if (m.own) return isHost ? t('room.papel.anfitriao') : null
    const p = (m.from && peers.find((x) => x.peerId === m.from)) || peers.find((x) => x.username === m.username)
    if (!p) return null
    return p.host ? t('room.papel.anfitriao') : p.canAdmit ? t('room.papel.coAnfitriao') : null
  }

  function guardar() {
    const texto = chatEmTexto(
      t('room.chat.exportTitulo', { code }),
      chat.messages,
      tools.polls
        .filter((p) => tools.pollSeenAt[p.id] != null)
        .map((p) => ({ at: tools.pollSeenAt[p.id], question: p.question, options: p.options, counts: p.counts })),
      locale,
      t('room.sondagens.sondagem'),
    )
    const url = URL.createObjectURL(new Blob([texto], { type: 'text/plain;charset=utf-8' }))
    const a = document.createElement('a')
    a.href = url
    a.download = nomeFicheiroChat(code, new Date())
    document.body.appendChild(a)
    a.click()
    a.remove()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }

  const vazio = itens.length === 0

  return (
    <div className="rm-chat">
      <div className="rm-chat__list" ref={listRef}>
        {vazio && (
          <div className="rm-panel__empty">
            <strong>{t('room.chat.vazio')}</strong>
            <span className="dx-muted">{t('room.chat.vazioTexto')}</span>
          </div>
        )}
        {itens.map((it, i) => {
          if (it.kind === 'poll') {
            const poll = tools.polls.find((p) => p.id === it.id)
            if (!poll) return null
            return (
              <div key={`poll-${it.id}`} className="rm-chat__poll">
                <PollCard
                  poll={poll}
                  myVote={tools.myVotes[poll.id]}
                  isHost={isHost}
                  onVote={(o) => tools.vote(poll.id, o)}
                  onClose={() => tools.closePoll(poll.id)}
                  present={presentes}
                  compact
                />
              </div>
            )
          }
          const m = it.msg
          const anterior = itens.slice(0, i).reverse().find((x) => x.kind === 'msg') as Extract<Item, { kind: 'msg' }> | undefined
          const role = papel(m)
          return (
            <Fragment key={m.id}>
              {!m.historical && anterior?.msg.historical && <div className="rm-chat__divider">{t('room.chat.inicioSessao')}</div>}
              <div className={cx('rm-chat__msg', m.own && 'is-own')}>
                <Avatar name={m.username} size={26} />
                <div className="rm-chat__content">
                  <span className="rm-chat__who">
                    {m.username}
                    {m.own && <span className="dx-muted"> · {t('room.tile.tu')}</span>}
                    <time className="rm-chat__time dx-num" dateTime={new Date(m.at).toISOString()}>
                      {new Date(m.at).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}
                    </time>
                    {role && <span className="rm-chat__role">{role}</span>}
                  </span>
                  <p className="rm-chat__bubble">
                    <ChatText text={m.text} />
                  </p>
                </div>
              </div>
            </Fragment>
          )
        })}
      </div>
      <div className="rm-chat__compose">
        {blocked && <Alert tone="warning">{t('room.chat.fechadoPeloAnfitriao')}</Alert>}
        {chat.mentionSuggestions.length > 0 && (
          <div className="rm-chat__mentions" role="listbox" aria-label={t('room.chat.mencionar')}>
            {chat.mentionSuggestions.map((name) => (
              <button
                key={name}
                type="button"
                role="option"
                aria-selected={false}
                onMouseDown={(e) => {
                  e.preventDefault()
                  chat.completeMention(name)
                  inputRef.current?.focus()
                }}
              >
                @{name}
              </button>
            ))}
          </div>
        )}
        {emojiOpen && (
          <div className="rm-chat__emojis" role="group" aria-label={t('room.chat.emojis')}>
            {CHAT_EMOJIS.map((e) => (
              <button
                key={e}
                type="button"
                onClick={() => {
                  chat.appendToInput(e)
                  setEmojiOpen(false)
                  inputRef.current?.focus()
                }}
              >
                {e}
              </button>
            ))}
          </div>
        )}
        <div className="rm-chat__input">
          <textarea
            ref={inputRef}
            rows={1}
            value={chat.input}
            disabled={blocked}
            placeholder={t('room.chat.placeholder')}
            aria-label={t('room.chat.placeholder')}
            onChange={(e) => chat.setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                chat.send()
                setEmojiOpen(false)
              } else if (e.key === 'Escape' && (emojiOpen || chat.mentionSuggestions.length > 0)) {
                e.preventDefault()
                e.stopPropagation()
                setEmojiOpen(false)
                chat.clearMention()
              } else if (e.key === 'Tab' && chat.mentionSuggestions.length > 0) {
                e.preventDefault()
                chat.completeMention(chat.mentionSuggestions[0])
              }
            }}
          />
          <IconButton icon="smile" bare label={t('room.chat.emojis')} aria-pressed={emojiOpen} onClick={() => setEmojiOpen((v) => !v)} disabled={blocked} />
          <button
            type="button"
            className="rm-chat__send"
            onClick={() => {
              chat.send()
              setEmojiOpen(false)
            }}
            disabled={blocked || !chat.input.trim()}
            aria-label={t('room.chat.enviar')}
            title={t('room.chat.enviar')}
          >
            <Icon name="send" size={14} />
          </button>
        </div>
        <div className="rm-chat__shortcuts">
          {isHost && (
            <button type="button" className="rm-chat__shortcut" onClick={onNewPoll}>
              <Icon name="poll" size={12} />
              {t('room.chat.novaSondagem')}
            </button>
          )}
          <button
            type="button"
            className="rm-chat__shortcut"
            onClick={guardar}
            disabled={vazio}
            title={t('room.chat.guardarChatDica')}
          >
            <Icon name="download" size={12} />
            {t('room.chat.guardarChat')}
          </button>
        </div>
      </div>
    </div>
  )
}
