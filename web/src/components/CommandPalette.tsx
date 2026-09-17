/**
 * Pesquisa global (Ctrl/Cmd+K) — comandos da app + a pesquisa PROFUNDA do
 * servidor (GET /api/search, contrato docs/reference/pesquisa.md §1):
 * reuniões (título, descrição, acta), gravações (título, transcrição,
 * capítulos e comentários com marca temporal), pessoas, quadros, salas,
 * mensagens de chat, destinos de emissão e webhooks. O servidor aplica a mesma
 * visibilidade do endpoint normal; aqui só se mostra o que ele devolve.
 *
 * - Os comandos (ir para um ecrã, nova reunião, entrar por código, tema…)
 *   ficam no cliente, como o contrato manda.
 * - O realce vem em SEGMENTOS (`highlight`): cada `text` é texto React (nunca
 *   HTML) e os `match` ficam em `<mark>`.
 * - Teclado: ↑/↓ entre linhas, Tab/Shift+Tab (ou Ctrl+↓/↑) saltam de grupo,
 *   Enter abre, Esc fecha e devolve o foco a quem o tinha.
 * - «Ver todos em <ecrã>» abre a lista desse recurso já com a pesquisa.
 * - Sem `/api/search` no servidor (404) diz-se isso — não se finge a pesquisa
 *   profunda filtrando listas no browser.
 */
import { ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorMessage, createRoom, globalSearch, GlobalSearchResult, HighlightSegment, isAbort, SearchHit, SearchType, User } from '../api'
import { parseRoomCode } from '../roomCode'
import { Icon, IconName } from '../ui/icons'
import { cx } from '../ui/kit'
import { clearRecent, hitHash, moreHash, pushRecent, readRecent } from './paletteRoutes'
import type { NavKey } from './shellContext'

interface Row {
  id: string
  label: ReactNode
  /** Nome acessível e texto do filtro dos comandos. */
  text: string
  hint?: ReactNode
  icon: IconName
  group: string
  run: () => void | Promise<void>
  kind?: 'more' | 'recent'
}

const TYPE_ICON: Record<SearchType, IconName> = {
  meetings: 'calendar',
  recordings: 'film',
  people: 'user',
  whiteboards: 'board',
  rooms: 'door',
  messages: 'chat',
  stream_destinations: 'live',
  webhooks: 'share',
  audit_events: 'shield',
}

/** Onde o realce é um EXCERTO (vai por baixo do título) e não o próprio título. */
const SNIPPET_IN = ['transcript', 'message', 'comment', 'chapter', 'description', 'minutes']

const MIN_CHARS = 2
const DEBOUNCE_MS = 200
const KEEP_OPEN = Symbol('keep-open')

type DeepSearch =
  | { s: 'idle' }
  | { s: 'loading'; q: string }
  | { s: 'ready'; q: string; d: GlobalSearchResult }
  | { s: 'error'; q: string; msg: string; unavailable: boolean }

export function Highlight({ segments, fallback }: { segments: HighlightSegment[] | undefined; fallback: string }) {
  if (!segments?.length) return <>{fallback}</>
  return (
    <>
      {segments.map((s, i) =>
        s.match ? (
          <mark key={i} className="palette__hl">
            {s.text}
          </mark>
        ) : (
          <span key={i}>{s.text}</span>
        ),
      )}
    </>
  )
}

export default function CommandPalette({
  onClose,
  onNavigate,
  onEnterRoom,
  onLogout,
  onOpenHash,
  onSettings,
  onToggleTheme,
  user,
  isAdmin,
}: {
  onClose: () => void
  onNavigate: (k: NavKey) => void
  onEnterRoom: (code: string) => void
  onLogout: () => void
  /** Abre uma rota (`/calendar/m/<id>`); dentro de uma reunião, num separador novo. */
  onOpenHash: (hash: string) => void
  onSettings?: () => void
  onToggleTheme: () => void
  user: User
  isAdmin: boolean
  inRoom?: boolean
}) {
  const { t, i18n } = useTranslation()
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const [err, setErr] = useState<string | null>(null)
  const [deep, setDeep] = useState<DeepSearch>({ s: 'idle' })
  const [nonce, setNonce] = useState(0)
  const [recent, setRecent] = useState<string[]>(() => readRecent(user.id))
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLUListElement>(null)
  const needle = q.trim()
  const code = parseRoomCode(q)
  const wantsSearch = needle.length >= MIN_CHARS && !code && /[\p{L}\p{N}]/u.test(needle)

  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null
    inputRef.current?.focus()
    return () => prev?.focus?.()
  }, [])

  // Pesquisa profunda: 200 ms depois de parar de escrever; a anterior aborta.
  useEffect(() => {
    if (!wantsSearch) {
      setDeep({ s: 'idle' })
      return
    }
    const ctrl = new AbortController()
    const id = window.setTimeout(() => {
      setDeep((prev) => (prev.s === 'ready' ? prev : { s: 'loading', q: needle }))
      globalSearch(needle.slice(0, 200), { limit: 5 }, ctrl.signal)
        .then((d) => setDeep({ s: 'ready', q: needle, d }))
        .catch((e) => {
          if (isAbort(e)) return
          const unavailable = e instanceof ApiError && e.status === 404
          setDeep({ s: 'error', q: needle, unavailable, msg: apiErrorMessage(e, t('search.global.erro')) })
        })
    }, DEBOUNCE_MS)
    return () => {
      window.clearTimeout(id)
      ctrl.abort()
    }
  }, [needle, wantsSearch, nonce, t])

  const rows = useMemo<Row[]>(() => {
    const cmdGroup = t('search.global.comandos')
    const cmd = (id: string, text: string, icon: IconName, run: Row['run'], hint?: string): Row => ({ id, label: text, text, icon, group: cmdGroup, run, hint })
    const nav = (key: NavKey, label: string, icon: IconName) => cmd(`nav-${key}`, label, icon, () => onNavigate(key), t('shell.paleta.irPara'))
    const list: Row[] = []
    if (code) list.push(cmd('join', t('shell.paleta.entrarEm', { codigo: code }), 'door', () => onEnterRoom(code)))
    list.push(
      cmd(
        'new',
        t('shell.paleta.novaReuniao'),
        'video',
        async () => {
          const room = await createRoom(t('shell.paleta.reuniaoDe', { nome: user.username }))
          onEnterRoom(room.code)
        },
        t('shell.paleta.salaPessoal'),
      ),
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
    if (onSettings) list.push(cmd('settings', t('shell.definicoes'), 'sliders', onSettings))
    list.push(cmd('theme', t('shell.paleta.alternarTema'), 'moon', onToggleTheme), cmd('logout', t('shell.terminarSessao'), 'logout', onLogout))

    // Sem texto: as pesquisas recentes primeiro, depois os comandos.
    if (!needle) {
      const rec = recent.map(
        (r, i): Row => ({
          id: `recent-${i}`,
          label: r,
          text: r,
          icon: 'clock',
          group: t('search.global.recentes'),
          kind: 'recent',
          run: () => {
            setQ(r)
            throw KEEP_OPEN
          },
        }),
      )
      return [...rec, ...list]
    }
    if (code) return list
    const norm = (v: string) => v.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
    const out = list.filter((c) => norm(c.text).includes(norm(needle)))
    if (deep.s !== 'ready' || deep.q !== needle) return out

    for (const g of deep.d.groups) {
      if (!g.items.length) continue
      const groupName = t(`search.global.tipos.${g.type}`, { defaultValue: g.type })
      for (const hit of g.items) {
        const hash = hitHash(hit)
        const titleHl = SNIPPET_IN.includes(hit.matched_in) ? undefined : hit.highlight
        out.push({
          id: `${g.type}-${hit.id}`,
          label: <Highlight segments={titleHl} fallback={hit.title} />,
          text: hit.title,
          hint: <HitHint hit={hit} locale={i18n.language} />,
          icon: TYPE_ICON[g.type] ?? 'search',
          group: groupName,
          run: () => {
            setRecent(pushRecent(user.id, needle))
            if (hit.type === 'rooms' && hit.target.room_code) onEnterRoom(String(hit.target.room_code))
            else if (hash) onOpenHash(hash)
          },
        })
      }
      const more = moreHash(g.type, needle)
      if (more && g.count > g.items.length) {
        const label = t('search.global.verTodos', { count: g.count, ecra: groupName }) + (g.count_kind === 'at_least' ? '+' : '')
        out.push({
          id: `more-${g.type}`,
          label,
          text: label,
          icon: 'arrowRight',
          group: groupName,
          kind: 'more',
          run: () => {
            setRecent(pushRecent(user.id, needle))
            onOpenHash(more)
          },
        })
      }
    }
    return out
  }, [needle, code, t, isAdmin, user.username, user.id, onNavigate, onEnterRoom, onSettings, onToggleTheme, onLogout, onOpenHash, deep, recent, i18n.language])

  useEffect(() => setSel(0), [needle, deep.s])
  useEffect(() => {
    listRef.current?.querySelector(`[data-row="${sel}"]`)?.scrollIntoView?.({ block: 'nearest' })
  }, [sel])

  async function run(r: Row | undefined) {
    if (!r) return
    setErr(null)
    try {
      await r.run()
      onClose()
    } catch (e) {
      if (e === KEEP_OPEN) {
        inputRef.current?.focus()
        return
      }
      setErr(apiErrorMessage(e, t('ui.erroGenerico')))
    }
  }

  /** Primeira linha do grupo seguinte (ou do anterior), em ciclo. */
  function jumpGroup(dir: 1 | -1) {
    if (!rows.length) return
    const current = rows[sel]?.group
    const starts = rows.map((r, i) => (i === 0 || rows[i - 1].group !== r.group ? i : -1)).filter((i) => i >= 0)
    const mine = starts.filter((i) => rows[i].group === current).pop() ?? 0
    const k = starts.indexOf(mine)
    setSel(starts[(k + dir + starts.length) % starts.length])
  }

  const skipped = deep.s === 'ready' && deep.q === needle ? deep.d.skipped : []
  const searching = wantsSearch && !(deep.s === 'ready' && deep.q === needle) && !(deep.s === 'error' && deep.q === needle)
  const deepEmpty = deep.s === 'ready' && deep.q === needle && deep.d.groups.every((g) => g.items.length === 0)
  const activeId = rows[sel] ? `cmd-${rows[sel].id}` : undefined

  return (
    <div className="dx-dialog-scrim palette-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label={t('shell.paleta.rotulo')} data-testid="palette">
        <div className="palette__search">
          <Icon name="search" />
          <input
            ref={inputRef}
            value={q}
            role="combobox"
            aria-expanded={rows.length > 0}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t('search.global.placeholder')}
            aria-label={t('search.global.placeholder')}
            aria-controls="palette-list"
            aria-autocomplete="list"
            aria-activedescendant={activeId}
            onKeyDown={(e) => {
              if (e.key === 'Tab' || ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && (e.ctrlKey || e.metaKey))) {
                e.preventDefault()
                jumpGroup(e.key === 'ArrowUp' || e.shiftKey ? -1 : 1)
              } else if (e.key === 'ArrowDown') {
                e.preventDefault()
                setSel((s) => Math.min(s + 1, rows.length - 1))
              } else if (e.key === 'ArrowUp') {
                e.preventDefault()
                setSel((s) => Math.max(s - 1, 0))
              } else if (e.key === 'Enter') {
                e.preventDefault()
                void run(rows[sel])
              } else if (e.key === 'Escape') {
                // A sala e as gavetas também ouvem Esc na janela: fecha-se SÓ a paleta.
                e.preventDefault()
                e.stopPropagation()
                onClose()
              }
            }}
          />
          {searching && <span className="dx-spinner" aria-hidden="true" />}
          <kbd className="dx-num">Esc</kbd>
        </div>
        {err && (
          <div className="palette__err" role="alert">
            {err}
          </div>
        )}
        <ul id="palette-list" ref={listRef} className="palette__list" role="listbox" aria-label={t('shell.paleta.rotulo')}>
          {rows.map((r, i) => {
            const first = r.group !== rows[i - 1]?.group
            return (
              <PaletteRow
                key={r.id}
                header={first ? r.group : null}
                action={
                  first && r.kind === 'recent' ? (
                    <button
                      type="button"
                      className="palette__clear"
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => {
                        clearRecent(user.id)
                        setRecent([])
                      }}
                    >
                      {t('search.global.limparRecentes')}
                    </button>
                  ) : null
                }
              >
                <li
                  id={`cmd-${r.id}`}
                  data-row={i}
                  data-kind={r.kind}
                  role="option"
                  aria-selected={i === sel}
                  className={cx('palette__item', i === sel && 'palette__item--sel', r.kind === 'more' && 'palette__item--more')}
                  onMouseEnter={() => setSel(i)}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => void run(r)}
                >
                  <Icon name={r.icon} />
                  <span className="palette__label">{r.label}</span>
                  {r.hint && <span className="dx-muted palette__hint">{r.hint}</span>}
                </li>
              </PaletteRow>
            )
          })}
        </ul>
        <div className="palette__status" role="status" aria-live="polite" data-testid="palette-status">
          {needle.length > 0 && needle.length < MIN_CHARS && !code && t('search.global.minimo', { n: MIN_CHARS })}
          {searching && t('search.global.aProcurar')}
          {deep.s === 'error' && deep.q === needle && (
            <span className={deep.unavailable ? undefined : 'palette__err-inline'}>
              {deep.unavailable ? t('search.global.indisponivel') : deep.msg}{' '}
              {!deep.unavailable && (
                <button type="button" className="palette__clear" onClick={() => setNonce((n) => n + 1)}>
                  {t('ui.tentarDeNovo')}
                </button>
              )}
            </span>
          )}
          {deepEmpty && t('search.global.semResultados', { q: needle })}
          {deep.s === 'ready' && deep.q === needle && !deepEmpty && <span className="dx-num">{t('search.global.demora', { ms: deep.d.took_ms })}</span>}
          {skipped.length > 0 && <span> · {t('search.global.omitidos', { count: skipped.length })}</span>}
        </div>
      </div>
    </div>
  )
}

function HitHint({ hit, locale }: { hit: SearchHit; locale: string }) {
  const { t } = useTranslation()
  const at = typeof hit.target.at_secs === 'number' ? hit.target.at_secs : null
  const where = SNIPPET_IN.includes(hit.matched_in) || ['email', 'code', 'url_host', 'action'].includes(hit.matched_in) ? t(`search.global.onde.${hit.matched_in}`, { defaultValue: '' }) : ''
  const date = hit.occurred_at ? new Date(hit.occurred_at).toLocaleDateString(locale, { day: 'numeric', month: 'short', year: 'numeric' }) : null
  const meta = [hit.subtitle, where, at !== null ? t('search.global.aos', { tempo: clock(at) }) : null, hit.subtitle ? null : date].filter(Boolean).join(' · ')
  return (
    <>
      {SNIPPET_IN.includes(hit.matched_in) && hit.highlight?.length ? (
        <span className="palette__snippet">
          <Highlight segments={hit.highlight} fallback="" />
        </span>
      ) : null}
      {meta && <span className="palette__meta dx-num">{meta}</span>}
    </>
  )
}

function clock(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const ss = String(s % 60).padStart(2, '0')
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`
}

/** Linha com o título do grupo por cima, quando o grupo muda. */
function PaletteRow({ header, action, children }: { header: string | null; action?: ReactNode; children: ReactNode }) {
  return (
    <>
      {header && (
        <li role="presentation" className="dx-eyebrow palette__group">
          <span>{header}</span>
          {action}
        </li>
      )}
      {children}
    </>
  )
}
