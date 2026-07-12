import { ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createRoom } from '../api'
import type { NavKey } from './Shell'
import { CalendarIcon, ClockIcon, FilmIcon, HomeIcon, NoteIcon, PeopleIcon, StageIcon } from '../icons'

// Command palette (Cmd/Ctrl-K) — navegação + ações rápidas, estilo Teams/Slack.
// Aberto por atalho global ou pelo botão de pesquisa da sidebar (ver Shell).

type Cmd = {
  id: string
  label: string
  keywords?: string
  icon?: ReactNode
  group: 'nav' | 'action'
  run: () => void
}

interface Props {
  open: boolean
  onClose: () => void
  onNavigate: (k: NavKey) => void
  onEnterRoom: (code: string, voice?: boolean) => void
  onLogout: () => void
  username: string
  isAdmin: boolean
}

export default function CommandPalette({ open, onClose, onNavigate, onEnterRoom, onLogout, username, isAdmin }: Props) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [sel, setSel] = useState(0)
  const [busy, setBusy] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const go = (k: NavKey) => { onClose(); onNavigate(k) }

  async function newMeeting() {
    if (busy) return
    setBusy(true)
    try {
      const room = await createRoom(`${t('home.meetingLabel', 'Reunião')} de ${username}`)
      onClose()
      onEnterRoom(room.code)
    } finally {
      setBusy(false)
    }
  }

  const commands: Cmd[] = useMemo(() => {
    const nav: Cmd[] = [
      { id: 'home', label: t('nav.home'), keywords: 'inicio dashboard painel', icon: <HomeIcon />, group: 'nav', run: () => go('home') },
      { id: 'calendar', label: t('nav.calendar'), keywords: 'agenda reunioes meetings', icon: <CalendarIcon />, group: 'nav', run: () => go('calendar') },
      { id: 'directory', label: t('nav.org'), keywords: 'organizacao diretorio contactos equipa', icon: <PeopleIcon />, group: 'nav', run: () => go('directory') },
      { id: 'recordings', label: t('nav.recordings'), keywords: 'gravacoes atas mom', icon: <FilmIcon />, group: 'nav', run: () => go('recordings') },
      { id: 'whiteboards', label: t('nav.whiteboards'), keywords: 'quadros whiteboard', icon: <NoteIcon />, group: 'nav', run: () => go('whiteboards') },
      ...(isAdmin
        ? [{ id: 'analytics', label: t('nav.analytics'), keywords: 'analises kpis admin', icon: <ClockIcon />, group: 'nav' as const, run: () => go('analytics') }]
        : []),
    ]
    const actions: Cmd[] = [
      { id: 'new', label: t('cmd.newMeeting', 'Nova reunião'), keywords: 'criar sala reunir agora call', icon: <StageIcon />, group: 'action', run: () => void newMeeting() },
      { id: 'tour', label: t('tour.replay', 'Ver introdução'), keywords: 'ajuda tour onboarding intro', group: 'action', run: () => { onClose(); window.dispatchEvent(new Event('dx-start-tour')) } },
      { id: 'logout', label: t('nav.logout'), keywords: 'sair terminar sessao', group: 'action', run: () => { onClose(); onLogout() } },
    ]
    return [...nav, ...actions]
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, isAdmin, username, busy])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return commands
    return commands.filter((c) => c.label.toLowerCase().includes(q) || (c.keywords || '').includes(q))
  }, [query, commands])

  // Reset ao abrir + foco no input.
  useEffect(() => {
    if (open) { setQuery(''); setSel(0); setTimeout(() => inputRef.current?.focus(), 20) }
  }, [open])
  useEffect(() => { setSel(0) }, [query])

  // Teclado.
  useEffect(() => {
    if (!open) return
    const on = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); onClose() }
      else if (e.key === 'ArrowDown') { e.preventDefault(); setSel((s) => Math.min(filtered.length - 1, s + 1)) }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setSel((s) => Math.max(0, s - 1)) }
      else if (e.key === 'Enter') { e.preventDefault(); filtered[sel]?.run() }
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  }, [open, filtered, sel, onClose])

  useEffect(() => {
    listRef.current?.querySelector('.cmd-item.sel')?.scrollIntoView({ block: 'nearest' })
  }, [sel])

  if (!open) return null

  let idx = -1
  const groups: { key: 'nav' | 'action'; label: string }[] = [
    { key: 'nav', label: t('cmd.navigate', 'Ir para') },
    { key: 'action', label: t('cmd.actions', 'Ações') },
  ]

  return (
    <div className="cmd-backdrop" onMouseDown={onClose} role="dialog" aria-modal="true" aria-label={t('cmd.title', 'Comandos')}>
      <div className="cmd-palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="cmd-search">
          <span className="cmd-search-icon" aria-hidden="true">⌕</span>
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('cmd.placeholder', 'Procurar páginas e ações…')}
            aria-label={t('cmd.placeholder', 'Procurar páginas e ações…')}
          />
          <kbd className="cmd-esc">Esc</kbd>
        </div>
        <div className="cmd-list" ref={listRef}>
          {filtered.length === 0 && <div className="cmd-empty">{t('cmd.empty', 'Sem resultados')}</div>}
          {groups.map((g) => {
            const items = filtered.filter((c) => c.group === g.key)
            if (items.length === 0) return null
            return (
              <div className="cmd-group" key={g.key}>
                <div className="cmd-group-label">{g.label}</div>
                {items.map((c) => {
                  idx++
                  const i = idx
                  return (
                    <button
                      key={c.id}
                      className={i === sel ? 'cmd-item sel' : 'cmd-item'}
                      onMouseEnter={() => setSel(i)}
                      onClick={() => c.run()}
                    >
                      <span className="cmd-item-icon">{c.icon ?? <span className="cmd-dot" />}</span>
                      <span className="cmd-item-label">{c.label}</span>
                      {i === sel && <span className="cmd-item-enter" aria-hidden="true">↵</span>}
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
