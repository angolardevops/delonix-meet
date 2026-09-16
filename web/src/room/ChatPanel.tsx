import { Fragment, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Avatar, IconButton, cx } from '../ui/kit'
import { CHAT_EMOJIS, type Chat } from './useChat'

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

export function ChatPanel({ chat, isHost }: { chat: Chat; isHost: boolean }) {
  const { t } = useTranslation()
  const [emojiOpen, setEmojiOpen] = useState(false)
  const listRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const blocked = !chat.chatOn && !isHost

  // Segue a conversa: a mensagem nova fica à vista.
  useEffect(() => {
    const el = listRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [chat.messages.length])

  return (
    <div className="rm-chat">
      <p className="rm-panel__note">
        <Icon name="info" size={12} />
        {t('room.chat.guardadas')}
      </p>
      <div className="rm-chat__list" ref={listRef}>
        {chat.messages.length === 0 && (
          <div className="rm-panel__empty">
            <strong>{t('room.chat.vazio')}</strong>
            <span className="dx-muted">{t('room.chat.vazioTexto')}</span>
          </div>
        )}
        {chat.messages.map((m, i) => (
          <Fragment key={m.id}>
            {!m.historical && i > 0 && chat.messages[i - 1].historical && (
              <div className="rm-chat__divider">{t('room.chat.inicioSessao')}</div>
            )}
            <div className={cx('rm-chat__msg', m.own && 'is-own')}>
              <Avatar name={m.username} size={26} />
              <div className="rm-chat__content">
                <span className="rm-chat__who">
                  {m.username}
                  {m.own && <span className="dx-muted"> · {t('room.tile.tu')}</span>}
                </span>
                <p className="rm-chat__bubble">
                  <ChatText text={m.text} />
                </p>
              </div>
            </div>
          </Fragment>
        ))}
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
      </div>
    </div>
  )
}
