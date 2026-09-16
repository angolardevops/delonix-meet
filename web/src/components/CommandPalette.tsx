/**
 * Paleta de comandos (Ctrl/Cmd+K): ir para um ecrã, abrir uma reunião nova,
 * entrar numa sala colando o código ou o link. Teclado primeiro — setas,
 * Enter, Esc.
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom, User } from '../api'
import { parseRoomCode } from '../roomCode'
import { Icon, IconName } from '../ui/icons'
import { cx } from '../ui/kit'
import type { NavKey } from './shellContext'

interface Command {
  id: string
  label: string
  hint?: string
  icon: IconName
  run: () => void | Promise<void>
}

export default function CommandPalette({
  onClose,
  onNavigate,
  onEnterRoom,
  onLogout,
  onSettings,
  onToggleTheme,
  user,
  isAdmin,
}: {
  onClose: () => void
  onNavigate: (k: NavKey) => void
  onEnterRoom: (code: string) => void
  onLogout: () => void
  onSettings: () => void
  onToggleTheme: () => void
  user: User
  isAdmin: boolean
}) {
  const { t } = useTranslation()
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const [err, setErr] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null
    inputRef.current?.focus()
    return () => prev?.focus?.()
  }, [])

  const commands = useMemo<Command[]>(() => {
    const nav = (key: NavKey, label: string, icon: IconName): Command => ({
      id: `nav-${key}`,
      label,
      hint: t('shell.paleta.irPara'),
      icon,
      run: () => onNavigate(key),
    })
    const list: Command[] = []
    const code = parseRoomCode(q)
    if (code) {
      list.push({ id: 'join', label: t('shell.paleta.entrarEm', { codigo: code }), icon: 'door', run: () => onEnterRoom(code) })
    }
    list.push(
      {
        id: 'new',
        label: t('shell.paleta.novaReuniao'),
        hint: t('shell.paleta.salaPessoal'),
        icon: 'video',
        run: async () => {
          const room = await createRoom(t('shell.paleta.reuniaoDe', { nome: user.username }))
          onEnterRoom(room.code)
        },
      },
      nav('home', t('shell.nav.inicio'), 'home'),
      nav('calendar', t('shell.nav.agenda'), 'calendar'),
      nav('studio', t('shell.nav.estudio'), 'live'),
      nav('recordings', t('shell.nav.gravacoes'), 'film'),
      nav('whiteboards', t('shell.nav.quadros'), 'board'),
      nav('directory', t('shell.nav.contactos'), 'people'),
    )
    if (isAdmin) {
      list.push(
        nav('integrations', t('shell.nav.integracoes'), 'plug'),
        nav('analytics', t('shell.nav.analise'), 'chart'),
        nav('admin', t('shell.nav.administracao'), 'building'),
      )
    }
    list.push(
      { id: 'settings', label: t('shell.definicoes'), icon: 'sliders', run: onSettings },
      { id: 'theme', label: t('shell.paleta.alternarTema'), icon: 'moon', run: onToggleTheme },
      { id: 'logout', label: t('shell.terminarSessao'), icon: 'logout', run: onLogout },
    )
    const needle = q.trim().toLowerCase()
    if (!needle || code) return list
    return list.filter((c) => c.label.toLowerCase().includes(needle))
  }, [q, t, isAdmin, user.username, onNavigate, onEnterRoom, onSettings, onToggleTheme, onLogout])

  useEffect(() => setSel(0), [q])

  async function run(c: Command | undefined) {
    if (!c) return
    setErr(null)
    try {
      await c.run()
      onClose()
    } catch (e) {
      setErr(apiErrorMessage(e, t('ui.erroGenerico')))
    }
  }

  return (
    <div className="dx-dialog-scrim palette-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label={t('shell.paleta.rotulo')}>
        <div className="palette__search">
          <Icon name="search" />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t('shell.paleta.placeholder')}
            aria-label={t('shell.paleta.placeholder')}
            aria-controls="palette-list"
            aria-activedescendant={commands[sel] ? `cmd-${commands[sel].id}` : undefined}
            onKeyDown={(e) => {
              if (e.key === 'ArrowDown') {
                e.preventDefault()
                setSel((s) => Math.min(s + 1, commands.length - 1))
              } else if (e.key === 'ArrowUp') {
                e.preventDefault()
                setSel((s) => Math.max(s - 1, 0))
              } else if (e.key === 'Enter') {
                e.preventDefault()
                void run(commands[sel])
              } else if (e.key === 'Escape') {
                e.preventDefault()
                onClose()
              }
            }}
          />
          <kbd className="dx-num">Esc</kbd>
        </div>
        {err && <div className="palette__err" role="alert">{err}</div>}
        <ul id="palette-list" className="palette__list" role="listbox">
          {commands.length === 0 && <li className="palette__empty">{t('shell.paleta.nada')}</li>}
          {commands.map((c, i) => (
            <li
              key={c.id}
              id={`cmd-${c.id}`}
              role="option"
              aria-selected={i === sel}
              className={cx('palette__item', i === sel && 'palette__item--sel')}
              onMouseEnter={() => setSel(i)}
              onClick={() => void run(c)}
            >
              <Icon name={c.icon} />
              <span style={{ flex: 1 }}>{c.label}</span>
              {c.hint && <span className="dx-muted dx-num">{c.hint}</span>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}
