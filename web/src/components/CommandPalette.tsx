/**
 * Paleta de comandos (Ctrl/Cmd+K): ir para um ecrã, abrir uma reunião nova,
 * entrar numa sala colando o código ou o link. Teclado primeiro — setas,
 * Enter, Esc.
 *
 * É também a pesquisa global do template («sessões, gravações, pessoas»):
 * com duas letras ou mais procura nas reuniões (listMeetings) e nas gravações
 * (recordingsLibrary) que a pessoa pode ver — carregadas uma vez, quando a
 * pesquisa começa — e nas pessoas das organizações dela (searchUsers, no
 * servidor, com o isolamento de lá). Abrir um resultado leva ao detalhe.
 */
import { ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom, isAbort, listMeetings, Meeting, recordingsLibrary, RecordingItem, searchUsers, User } from '../api'
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
  /** Resultados de pesquisa vêm agrupados; os comandos não têm grupo. */
  group?: 'meetings' | 'recordings' | 'people'
}

const MAX_PER_GROUP = 5

/** Filtra por todas as palavras (sem acentos, sem maiúsculas). */
export function matchesAll(haystack: string, needle: string): boolean {
  const norm = (v: string) => v.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
  const h = norm(haystack)
  return norm(needle)
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => h.includes(w))
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
  const { t, i18n } = useTranslation()
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const [err, setErr] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const [data, setData] = useState<{ meetings: Meeting[]; recordings: RecordingItem[] } | null>(null)
  const [people, setPeople] = useState<{ q: string; users: User[] } | null>(null)
  const needle = q.trim()
  const searching = needle.length >= 2 && !parseRoomCode(q)

  // Reuniões e gravações: um pedido de cada, na primeira pesquisa.
  const wantData = searching && data === null
  useEffect(() => {
    if (!wantData) return
    const ctrl = new AbortController()
    Promise.all([
      listMeetings(ctrl.signal).catch((e) => (isAbort(e) ? Promise.reject(e) : [])),
      recordingsLibrary(ctrl.signal).catch((e) => (isAbort(e) ? Promise.reject(e) : [])),
    ])
      .then(([meetings, recordings]) => setData({ meetings, recordings }))
      .catch(() => {})
    return () => ctrl.abort()
  }, [wantData])

  // Pessoas: no servidor, 250 ms depois de parar de escrever.
  useEffect(() => {
    if (!searching) return
    let alive = true
    const id = setTimeout(() => {
      searchUsers(needle)
        .then((users) => alive && setPeople({ q: needle, users }))
        .catch(() => alive && setPeople({ q: needle, users: [] }))
    }, 250)
    return () => {
      alive = false
      clearTimeout(id)
    }
  }, [needle, searching])

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
        nav('ai', t('consola.nav.ia'), 'sparkles'),
      )
    }
    list.push(
      { id: 'settings', label: t('shell.definicoes'), icon: 'sliders', run: onSettings },
      { id: 'theme', label: t('shell.paleta.alternarTema'), icon: 'moon', run: onToggleTheme },
      { id: 'logout', label: t('shell.terminarSessao'), icon: 'logout', run: onLogout },
    )
    const term = q.trim().toLowerCase()
    if (!term || code) return list
    const found = list.filter((c) => c.label.toLowerCase().includes(term))
    if (!searching) return found

    const locale = i18n.language
    const day = (iso: string) => new Date(iso).toLocaleDateString(locale, { day: 'numeric', month: 'short' })
    const meetings: Command[] = (data?.meetings ?? [])
      .filter((m) => matchesAll(`${m.title} ${m.owner_name} ${m.room_code ?? ''}`, term))
      .sort((a, b) => b.starts_at.localeCompare(a.starts_at))
      .slice(0, MAX_PER_GROUP)
      .map((m) => ({
        id: `m-${m.id}`,
        group: 'meetings',
        label: m.title,
        hint: `${day(m.starts_at)} · ${new Date(m.starts_at).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}`,
        icon: m.kind === 'voice' ? 'phone' : 'calendar',
        run: () => {
          location.hash = `/calendar/m/${m.id}`
        },
      }))
    const recordings: Command[] = (data?.recordings ?? [])
      .filter((r) => matchesAll(`${r.filename} ${r.room_code} ${r.uploader_name}`, term))
      .sort((a, b) => b.created_at.localeCompare(a.created_at))
      .slice(0, MAX_PER_GROUP)
      .map((r) => ({
        id: `r-${r.id}`,
        group: 'recordings',
        label: r.filename.replace(/\.(webm|mp4|mkv)$/i, ''),
        hint: `${day(r.created_at)} · ${r.room_code}`,
        icon: 'film',
        run: () => {
          location.hash = `/recordings?id=${r.id}`
        },
      }))
    const persons: Command[] = (people?.q === needle ? people.users : []).slice(0, MAX_PER_GROUP).map((u) => ({
      id: `u-${u.id}`,
      group: 'people',
      label: u.username,
      hint: u.email,
      icon: 'user',
      run: () => {
        location.hash = `/directory?u=${u.id}`
      },
    }))
    return [...found, ...meetings, ...recordings, ...persons]
  }, [q, t, isAdmin, user.username, onNavigate, onEnterRoom, onSettings, onToggleTheme, onLogout, searching, data, people, needle, i18n.language])

  const loadingResults = searching && (data === null || people?.q !== needle)

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
            placeholder={t('consola.pesquisa.placeholder')}
            aria-label={t('consola.pesquisa.placeholder')}
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
          {commands.length === 0 && (
            <li className="palette__empty">{loadingResults ? t('consola.pesquisa.aProcurar') : t('shell.paleta.nada')}</li>
          )}
          {commands.map((c, i) => (
            <PaletteRow key={c.id} header={c.group && c.group !== commands[i - 1]?.group ? t(`consola.pesquisa.${c.group}`) : null}>
            <li
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
            </PaletteRow>
          ))}
          {commands.length > 0 && loadingResults && (
            <li className="palette__empty" role="presentation">
              {t('consola.pesquisa.aProcurar')}
            </li>
          )}
        </ul>
      </div>
    </div>
  )
}

/** Linha com o título do grupo por cima, quando o grupo muda. */
function PaletteRow({ header, children }: { header: string | null; children: ReactNode }) {
  return (
    <>
      {header && (
        <li role="presentation" className="dx-eyebrow" style={{ padding: '10px 10px 4px' }}>
          {header}
        </li>
      )}
      {children}
    </>
  )
}
