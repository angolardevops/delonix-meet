import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Avatar, IconButton, cx } from '../ui/kit'
import { chatEmTexto, nomeFicheiroChat } from './chatExport'
import { respostasPorMae } from './chatState'
import { rotuloPapel } from './ParticipantTile'
import { PollCard } from './PollCard'
import { QuestionCard } from './QuestionCard'
import { CHAT_EMOJIS, CHAT_REACTIONS, type Chat, type ChatMsg } from './useChat'
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

/** «Privada · para X» (ou «para ti») numa mensagem de conversa directa. */
function PrivadaChip({ m }: { m: ChatMsg }) {
  const { t } = useTranslation()
  return (
    <span className="rm-chat__private">
      <Icon name="lock" size={9} />
      {m.own ? t('room.chat.privadaPara', { nome: m.toUsername ?? '' }) : t('room.chat.privadaParaTi')}
    </span>
  )
}

type Item =
  | { kind: 'msg'; at: number; msg: ChatMsg; index: number }
  | { kind: 'poll'; at: number; id: string }
  | { kind: 'qa'; at: number; id: string }

export function ChatPanel({
  chat,
  isHost,
  code,
  peers,
  tools,
  onNewPoll,
  myRole,
}: {
  chat: Chat
  isHost: boolean
  code: string
  peers: RemotePeer[]
  tools: MeetingTools
  /** Atalho do anfitrião para o compositor de sondagens. */
  onNewPoll: () => void
  /** O meu papel (para o chip nas minhas mensagens). */
  myRole?: RemotePeer['role']
}) {
  const { t, i18n } = useTranslation()
  const locale = i18n.language === 'en' ? 'en-GB' : i18n.language === 'fr' ? 'fr-FR' : 'pt-PT'
  const [emojiOpen, setEmojiOpen] = useState(false)
  /** Fios fechados pela pessoa (abertos por omissão, como no template). */
  const [fechados, setFechados] = useState<Record<string, boolean>>({})
  const listRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const blocked = !chat.chatOn && !isHost
  const presentes = peers.length + 1

  // Abrir o chat é para escrever: o foco vai para o campo (sem isto, o que se
  // escrevia logo a seguir perdia-se).
  useEffect(() => {
    if (!blocked) inputRef.current?.focus()
  }, [blocked])

  const respostas = useMemo(() => respostasPorMae(chat.messages), [chat.messages])
  const ids = useMemo(() => new Set(chat.messages.map((m) => m.id).filter(Boolean)), [chat.messages])

  // O fio: mensagens (as respostas vão debaixo da mãe), sondagens e perguntas,
  // pela ordem em que chegaram.
  const itens = useMemo<Item[]>(() => {
    const out: Item[] = chat.messages
      .map((msg, index) => ({ kind: 'msg' as const, at: msg.at, msg, index }))
      // Resposta a uma mensagem que já não está no histórico: fica no fio.
      .filter((it) => !it.msg.replyTo || !ids.has(it.msg.replyTo))
    for (const p of tools.polls) {
      const at = tools.pollSeenAt[p.id]
      if (at != null) out.push({ kind: 'poll', at, id: p.id })
    }
    for (const q of tools.questions) {
      const at = tools.qaSeenAt[q.id]
      if (at != null) out.push({ kind: 'qa', at, id: q.id })
    }
    return out.sort((a, b) => a.at - b.at || (a.kind === 'msg' && b.kind === 'msg' ? a.index - b.index : 0))
  }, [chat.messages, ids, tools.polls, tools.pollSeenAt, tools.questions, tools.qaSeenAt])

  // Segue a conversa: o item novo fica à vista.
  const ultimo = chat.messages.length + tools.polls.length + tools.questions.length
  useEffect(() => {
    const el = listRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [ultimo])

  /** Papel de quem escreveu, lido de quem está na sala AGORA. */
  function papel(m: ChatMsg): string | null {
    if (m.own) return myRole ? rotuloPapel(t, myRole) : isHost ? t('room.papel.anfitriao') : null
    const p = (m.from && peers.find((x) => x.peerId === m.from)) || peers.find((x) => x.username === m.username)
    if (!p) return null
    return rotuloPapel(t, p.host ? 'host' : p.role)
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

  const hora = (at: number) => new Date(at).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })

  /** Reacções e acções de uma mensagem (as acções aparecem ao passar/focar). */
  function rodape(m: ChatMsg, filhas: ChatMsg[] | undefined) {
    const reaccoes = Object.entries(m.reactions).sort((a, b) => b[1] - a[1])
    const minhas = (m.id && chat.myReactions[m.id]) || []
    const aberto = m.id ? !fechados[m.id] : true
    return (
      <>
        {(reaccoes.length > 0 || (filhas && filhas.length > 0)) && (
          <div className="rm-chat__reacts">
            {reaccoes.map(([emoji, n]) => (
              <button
                key={emoji}
                type="button"
                className={cx('rm-chat__react', minhas.includes(emoji) && 'is-on')}
                aria-pressed={minhas.includes(emoji)}
                aria-label={t('room.chat.reaccao', { emoji, count: n })}
                disabled={!m.id || blocked}
                onClick={() => m.id && chat.react(m.id, emoji)}
              >
                {emoji} <span className="dx-num">{n}</span>
              </button>
            ))}
            {filhas && filhas.length > 0 && m.id && (
              <button
                type="button"
                className="rm-chat__thread"
                aria-expanded={aberto}
                onClick={() => setFechados((f) => ({ ...f, [m.id!]: aberto }))}
              >
                {t('room.chat.respostas', { count: filhas.length })}
              </button>
            )}
          </div>
        )}
        {m.id && !blocked && (
          <div className="rm-chat__actions" role="group" aria-label={t('room.chat.acoesMensagem', { nome: m.username })}>
            {CHAT_REACTIONS.map((e) => (
              <button key={e} type="button" aria-label={t('room.chat.reagir', { emoji: e })} title={t('room.chat.reagir', { emoji: e })} onClick={() => chat.react(m.id!, e)}>
                {e}
              </button>
            ))}
            <button
              type="button"
              className="rm-chat__replybtn"
              onClick={() => {
                // Responder a uma resposta continua o MESMO fio.
                chat.startReply(m.replyTo && ids.has(m.replyTo) ? chat.messages.find((x) => x.id === m.replyTo) ?? m : m)
                inputRef.current?.focus()
              }}
            >
              {t('room.chat.responder')}
            </button>
            {!m.own && !m.private && (
              <button
                type="button"
                className="rm-chat__replybtn"
                onClick={() => {
                  chat.replyPrivately(m)
                  inputRef.current?.focus()
                }}
              >
                {t('room.chat.responderPrivado')}
              </button>
            )}
          </div>
        )}
      </>
    )
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
          if (it.kind === 'qa') {
            const q = tools.questions.find((x) => x.id === it.id)
            if (!q) return null
            return <QuestionCard key={`qa-${it.id}`} q={q} tools={tools} isHost={isHost} />
          }
          const m = it.msg
          const anterior = itens.slice(0, i).reverse().find((x) => x.kind === 'msg') as Extract<Item, { kind: 'msg' }> | undefined
          const role = papel(m)
          const filhas = m.id ? respostas.get(m.id) : undefined
          const abertas = m.id ? !fechados[m.id] : false
          return (
            <Fragment key={m.key}>
              {!m.historical && anterior?.msg.historical && <div className="rm-chat__divider">{t('room.chat.inicioSessao')}</div>}
              <div className={cx('rm-chat__msg', m.own && 'is-own', m.pending && 'is-pending')}>
                <Avatar name={m.username} size={26} />
                <div className="rm-chat__content">
                  <span className="rm-chat__who">
                    <span className="rm-chat__name">{m.username}</span>
                    <time className="rm-chat__time dx-num" dateTime={new Date(m.at).toISOString()}>
                      {hora(m.at)}
                    </time>
                    {role && <span className="rm-chat__role">{role}</span>}
                    {m.private && <PrivadaChip m={m} />}
                    {m.pending && <span className="rm-chat__time">{t('room.chat.aEnviar')}</span>}
                  </span>
                  <p className={cx('rm-chat__bubble', m.private && 'is-private')}>
                    <ChatText text={m.text} />
                  </p>
                  {rodape(m, filhas)}
                </div>
              </div>
              {abertas &&
                filhas?.map((r) => (
                  <div key={r.key} className={cx('rm-chat__reply', r.pending && 'is-pending')}>
                    <Avatar name={r.username} size={22} />
                    <div className="rm-chat__content">
                      <span className="rm-chat__who">
                        <span className="rm-chat__name">{r.username}</span>
                        <time className="rm-chat__time dx-num" dateTime={new Date(r.at).toISOString()}>
                          {hora(r.at)}
                        </time>
                        {r.private && <PrivadaChip m={r} />}
                      </span>
                      <p className="rm-chat__replytext">
                        <ChatText text={r.text} />
                      </p>
                      {rodape(r, undefined)}
                    </div>
                  </div>
                ))}
            </Fragment>
          )
        })}
      </div>
      <div className="rm-chat__compose">
        {blocked && <Alert tone="warning">{t('room.chat.fechadoPeloAnfitriao')}</Alert>}
        {!blocked && (
          <div className="rm-chat__to">
            <label htmlFor="rm-chat-to" className="dx-muted">
              {t('room.chat.para')}
            </label>
            <select
              id="rm-chat-to"
              value={chat.target?.peerId ?? ''}
              // Uma resposta a uma privada fica no mesmo par: o «Para» não se muda a meio.
              disabled={!!chat.replyTo?.private}
              onChange={(e) => {
                const p = peers.find((x) => x.peerId === e.target.value)
                chat.setTarget(p ? { peerId: p.peerId, username: p.username } : null)
              }}
            >
              <option value="">{t('room.chat.paraTodos')}</option>
              {chat.target && !peers.some((p) => p.peerId === chat.target!.peerId) && <option value={chat.target.peerId}>{chat.target.username}</option>}
              {peers
                .filter((p) => !p.is_pstn && !p.is_bot)
                .map((p) => (
                  <option key={p.peerId} value={p.peerId}>
                    {p.username}
                  </option>
                ))}
            </select>
            {chat.target && <span className="rm-chat__todica">{t('room.chat.privadaDica', { nome: chat.target.username })}</span>}
          </div>
        )}
        {chat.replyTo && (
          <div className="rm-chat__replying">
            <Icon name="undo" size={11} />
            <span>{t('room.chat.aResponderA', { nome: chat.replyTo.username })}</span>
            <span className="dx-spacer" />
            <IconButton icon="x" bare label={t('room.chat.cancelarResposta')} onClick={() => chat.startReply(null)} />
          </div>
        )}
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
        <div className={cx('rm-chat__input', chat.target && 'is-private')}>
          <textarea
            ref={inputRef}
            rows={1}
            value={chat.input}
            disabled={blocked}
            placeholder={chat.target ? t('room.chat.placeholderPrivado', { nome: chat.target.username }) : t('room.chat.placeholder')}
            aria-label={chat.target ? t('room.chat.placeholderPrivado', { nome: chat.target.username }) : t('room.chat.placeholder')}
            onChange={(e) => chat.setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                chat.send()
                setEmojiOpen(false)
              } else if (e.key === 'Escape' && (emojiOpen || chat.mentionSuggestions.length > 0 || chat.replyTo)) {
                e.preventDefault()
                e.stopPropagation()
                setEmojiOpen(false)
                chat.clearMention()
                chat.startReply(null)
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
            <Icon name="arrowUp" size={12} />
          </button>
        </div>
        <div className="rm-chat__shortcuts">
          {isHost && (
            <button type="button" className="rm-chat__shortcut" onClick={onNewPoll}>
              {t('room.chat.novaSondagem')}
            </button>
          )}
          <button type="button" className="rm-chat__shortcut" onClick={guardar} disabled={vazio} title={t('room.chat.guardarChatDica')}>
            {t('room.chat.guardarChat')}
          </button>
        </div>
      </div>
    </div>
  )
}
