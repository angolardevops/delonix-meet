/**
 * Painel de pesquisa estilo Odoo — a caixa com facetas e o menu Filtros ·
 * Agrupar por · Favoritos. Um componente do kit: não sabe de que ecrã é; lê um
 * `SearchSchema` (do servidor, ou montado localmente com a mesma forma) e
 * devolve um `SearchState`.
 *
 * Teclado:
 *  - na caixa (combobox ARIA): escrever mostra sugestões («Pesquisar <campo>
 *    por: …»); ↑/↓ escolhem, Enter aplica, Esc fecha/limpa, Backspace com a
 *    caixa vazia remove a última faceta, ↓ com a caixa vazia abre o menu;
 *  - no menu: ↑/↓ dentro da coluna, ←/→ entre colunas, Esc fecha e devolve o
 *    foco ao botão.
 * Em ecrã estreito (≤ 600 px) o menu abre como folha inferior.
 */
import { KeyboardEvent, ReactNode, useEffect, useId, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { SavedSearch, SearchSchema } from '../../api'
import { Icon } from '../icons'
import { Button, Checkbox, cx, Spinner, TextInput } from '../kit'
import '../search.css'
import CustomFilterDialog from './CustomFilterDialog'
import { SchemaLabels, useSchemaLabels } from './labels'
import { norm } from './local'
import {
  addQ,
  addTerm,
  CustomFilter,
  facetRefs,
  lastFacet,
  MAX_GROUP_BY,
  removeFacet,
  SearchState,
  toggleFilter,
  toggleGroupBy,
} from './model'
import type { Favorites } from './useSearch'

interface Suggestion {
  id: string
  kind: string
  text: string
  apply: (s: SearchState) => SearchState
}

export default function SearchPanel({
  schema,
  state,
  onChange,
  favorites,
  label,
  placeholder,
  aside,
  className,
}: {
  schema: SearchSchema
  state: SearchState
  onChange: (s: SearchState) => void
  favorites: Favorites
  /** Nome acessível da caixa («Pesquisar gravações»). */
  label: string
  placeholder?: string
  /** À direita da caixa: o paginador, normalmente. */
  aside?: ReactNode
  className?: string
}) {
  const { t } = useTranslation()
  const L = useSchemaLabels(schema)
  const [text, setText] = useState('')
  const [active, setActive] = useState(0)
  const [menuOpen, setMenuOpen] = useState(false)
  const [custom, setCustom] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const toggleRef = useRef<HTMLButtonElement>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const listId = useId()
  const menuId = useId()

  const suggestions = useMemo(() => buildSuggestions(schema, text, L, t, state), [schema, text, L, t, state])
  useEffect(() => setActive(0), [text])
  const showList = text.trim().length > 0 && suggestions.length > 0

  const refs = facetRefs(state, schema)

  function commit(sg: Suggestion | undefined) {
    if (!sg) return
    onChange(sg.apply(state))
    setText('')
  }

  function onKey(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (!showList) {
        setMenuOpen(true)
        return
      }
      setActive((a) => Math.min(a + 1, suggestions.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActive((a) => Math.max(a - 1, 0))
    } else if (e.key === 'Enter') {
      if (showList) {
        e.preventDefault()
        commit(suggestions[active])
      }
    } else if (e.key === 'Escape') {
      if (text || menuOpen) {
        e.preventDefault()
        e.stopPropagation()
        setText('')
        setMenuOpen(false)
      }
    } else if (e.key === 'Backspace' && !text) {
      const last = lastFacet(state, schema)
      if (last) {
        e.preventDefault()
        onChange(removeFacet(state, last, schema))
      }
    }
  }

  // Clique fora fecha o menu (a folha inferior tem o seu fundo).
  useEffect(() => {
    if (!menuOpen) return
    const on = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setMenuOpen(false)
    }
    document.addEventListener('mousedown', on)
    return () => document.removeEventListener('mousedown', on)
  }, [menuOpen])

  function closeMenu() {
    setMenuOpen(false)
    toggleRef.current?.focus()
  }

  return (
    <div className={cx('dx-sp', className)} ref={rootRef} data-search-panel={schema.resource}>
      <div className="dx-sp__row">
        <div className={cx('dx-sp__box', (showList || menuOpen) && 'is-open')} onMouseDown={(e) => e.target === e.currentTarget && inputRef.current?.focus()}>
          <Icon name="search" size={14} />
          <ul className="dx-sp__facets" aria-label={t('search.painel.facetas')}>
            {refs.map((ref) => {
              const f = L.facet(ref, state)
              const k = ref.kind === 'term' ? `term-${ref.field}` : ref.kind === 'filters' ? `f-${ref.group}` : ref.kind === 'custom' ? `c-${ref.index}` : ref.kind
              return (
                <li key={k} className={cx('dx-sp__facet', `dx-sp__facet--${ref.kind}`)} data-facet={ref.kind}>
                  <span className="dx-sp__facet-kind">
                    <Icon name={ref.kind === 'groupBy' ? 'layers' : ref.kind === 'q' || ref.kind === 'term' ? 'search' : 'filter'} size={11} />
                    {f.kind}
                  </span>
                  <span className="dx-sp__facet-text">{f.text}</span>
                  <button
                    type="button"
                    className="dx-sp__facet-x"
                    aria-label={t('search.painel.removerFaceta', { faceta: `${f.kind}: ${f.text}` })}
                    onClick={() => {
                      onChange(removeFacet(state, ref, schema))
                      inputRef.current?.focus()
                    }}
                  >
                    <Icon name="x" size={11} />
                  </button>
                </li>
              )
            })}
          </ul>
          <input
            ref={inputRef}
            className="dx-sp__input"
            type="text"
            role="combobox"
            autoComplete="off"
            spellCheck={false}
            value={text}
            placeholder={refs.length ? undefined : (placeholder ?? t('search.painel.placeholder'))}
            aria-label={label}
            aria-expanded={showList}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={showList ? `${listId}-${active}` : undefined}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKey}
            onBlur={() => window.setTimeout(() => setText((v) => (document.activeElement === inputRef.current ? v : '')), 150)}
          />
          <button
            ref={toggleRef}
            type="button"
            className="dx-sp__toggle"
            aria-haspopup="dialog"
            aria-expanded={menuOpen}
            aria-controls={menuId}
            aria-label={t('search.painel.abrirOpcoes')}
            title={t('search.painel.abrirOpcoes')}
            onClick={() => setMenuOpen((o) => !o)}
          >
            <Icon name="chevronDown" size={14} />
          </button>
          {showList && (
            <ul id={listId} role="listbox" className="dx-sp__suggest" aria-label={t('search.painel.sugestoes')}>
              {suggestions.map((sg, i) => (
                <li
                  key={sg.id}
                  id={`${listId}-${i}`}
                  role="option"
                  aria-selected={i === active}
                  className={cx('dx-sp__opt', i === active && 'is-active')}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => commit(sg)}
                >
                  <span className="dx-muted">{sg.kind}</span> <strong>{sg.text}</strong>
                </li>
              ))}
            </ul>
          )}
        </div>
        {aside && <div className="dx-sp__aside">{aside}</div>}
      </div>

      {menuOpen && (
        <>
          <div className="dx-sp__scrim" aria-hidden="true" onClick={() => setMenuOpen(false)} />
          <SearchMenu
            id={menuId}
            schema={schema}
            state={state}
            onChange={onChange}
            favorites={favorites}
            labels={L}
            onClose={closeMenu}
            onCustom={() => {
              setMenuOpen(false)
              setCustom(true)
            }}
          />
        </>
      )}
      {custom && (
        <CustomFilterDialog
          schema={schema}
          labels={L}
          onClose={() => setCustom(false)}
          onApply={(f: CustomFilter) => {
            onChange({ ...state, custom: [...state.custom, f] })
            setCustom(false)
          }}
        />
      )}
    </div>
  )
}

function buildSuggestions(
  schema: SearchSchema,
  raw: string,
  L: SchemaLabels,
  t: (k: string, o?: Record<string, unknown>) => string,
  state: SearchState,
): Suggestion[] {
  const text = raw.trim()
  if (!text) return []
  const out: Suggestion[] = []
  const n = norm(text)
  if (schema.text_search.fields.length) {
    out.push({ id: 'q', kind: t('search.painel.pesquisarEmTudo'), text, apply: (s) => addQ(s, text) })
  }
  for (const f of schema.fields) {
    if (!f.filterable) continue
    if (f.type === 'text' && f.operators.includes('contains')) {
      out.push({ id: `t-${f.name}`, kind: t('search.painel.pesquisarCampo', { campo: L.field(f.name) }), text, apply: (s) => addTerm(s, f.name, text) })
    }
  }
  // Valores de enum cujo rótulo corresponde ao que se escreveu: «Estado: Falhada».
  for (const f of schema.fields) {
    if (!f.filterable || f.type !== 'enum') continue
    for (const o of f.options ?? []) {
      const lbl = L.option(f.name, o.value)
      if (norm(lbl).includes(n)) out.push({ id: `e-${f.name}-${o.value}`, kind: `${L.field(f.name)}:`, text: lbl, apply: (s) => addTerm(s, f.name, o.value) })
    }
  }
  for (const pf of schema.filters) {
    if (state.filters.includes(pf.name)) continue
    const lbl = L.filter(pf.name)
    if (norm(lbl).includes(n)) out.push({ id: `f-${pf.name}`, kind: `${t('search.painel.filtro')}:`, text: lbl, apply: (s) => toggleFilter(s, pf.name) })
  }
  return out.slice(0, 12)
}

// ---------------------------------------------------------------------- menu

function SearchMenu({
  id,
  schema,
  state,
  onChange,
  favorites,
  labels: L,
  onClose,
  onCustom,
}: {
  id: string
  schema: SearchSchema
  state: SearchState
  onChange: (s: SearchState) => void
  favorites: Favorites
  labels: SchemaLabels
  onClose: () => void
  onCustom: () => void
}) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const [openDate, setOpenDate] = useState<string | null>(null)

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[data-sp-nav]')?.focus()
  }, [])

  // Setas: ↑/↓ na coluna, ←/→ entre colunas.
  function onKey(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      onClose()
      return
    }
    const target = e.target as HTMLElement
    if (target.tagName === 'INPUT' && e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return
    if (!['ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight'].includes(e.key)) return
    const cols = [...(ref.current?.querySelectorAll<HTMLElement>('.dx-sp-col') ?? [])]
    const col = cols.findIndex((c) => c.contains(target))
    if (col < 0) return
    e.preventDefault()
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      const next = cols[(col + (e.key === 'ArrowRight' ? 1 : cols.length - 1)) % cols.length]
      next?.querySelector<HTMLElement>('[data-sp-nav]')?.focus()
      return
    }
    const items = [...cols[col].querySelectorAll<HTMLElement>('[data-sp-nav]')]
    const i = items.indexOf(target)
    items[Math.max(0, Math.min(items.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))]?.focus()
  }

  // Pré-definidos por grupo, pela ordem do schema; separador entre grupos.
  const groups: { group: string; names: string[] }[] = []
  for (const f of schema.filters) {
    const g = groups.find((x) => x.group === f.group)
    if (g) g.names.push(f.name)
    else groups.push({ group: f.group, names: [f.name] })
  }
  const groupable = schema.fields.filter((f) => f.groupable)

  return (
    <div id={id} ref={ref} className="dx-sp__menu" role="dialog" aria-label={t('search.painel.abrirOpcoes')} onKeyDown={onKey}>
      <section className="dx-sp-col" aria-labelledby={`${id}-f`}>
        <h3 id={`${id}-f`} className="dx-sp-col__title">
          <Icon name="filter" size={13} />
          {t('search.painel.filtros')}
        </h3>
        {groups.length === 0 && <p className="dx-sp-col__empty">{t('search.painel.semFiltros')}</p>}
        {groups.map((g, gi) => (
          <ul key={g.group} className={cx('dx-sp-col__list', gi > 0 && 'has-sep')} role="group">
            {g.names.map((name) => {
              const on = state.filters.includes(name)
              return (
                <li key={name}>
                  <button type="button" className="dx-sp-item" data-sp-nav aria-pressed={on} onClick={() => onChange(toggleFilter(state, name))}>
                    <span className="dx-sp-item__check" aria-hidden="true">{on && <Icon name="check" size={12} />}</span>
                    {L.filter(name)}
                  </button>
                </li>
              )
            })}
          </ul>
        ))}
        <ul className="dx-sp-col__list has-sep">
          <li>
            <button type="button" className="dx-sp-item dx-sp-item--action" data-sp-nav onClick={onCustom}>
              <Icon name="plus" size={12} />
              {t('search.painel.adicionarFiltro')}
            </button>
          </li>
        </ul>
      </section>

      <section className="dx-sp-col" aria-labelledby={`${id}-g`}>
        <h3 id={`${id}-g`} className="dx-sp-col__title">
          <Icon name="layers" size={13} />
          {t('search.painel.agruparPor')}
        </h3>
        {groupable.length === 0 && <p className="dx-sp-col__empty">{t('search.painel.semAgrupamentos')}</p>}
        <ul className="dx-sp-col__list">
          {groupable.map((f) => {
            const level = state.groupBy.findIndex((g) => g.split(':')[0] === f.name)
            const on = level >= 0
            const full = !on && state.groupBy.length >= MAX_GROUP_BY
            if (f.type === 'datetime' && f.granularities?.length) {
              const expanded = openDate === f.name
              return (
                <li key={f.name}>
                  <button
                    type="button"
                    className="dx-sp-item"
                    data-sp-nav
                    aria-expanded={expanded}
                    disabled={full}
                    onClick={() => setOpenDate(expanded ? null : f.name)}
                  >
                    <span className="dx-sp-item__check" aria-hidden="true">{on && <span className="dx-sp-item__lvl dx-num">{level + 1}</span>}</span>
                    {L.field(f.name)}
                    <Icon name={expanded ? 'chevronUp' : 'chevronDown'} size={12} />
                  </button>
                  {expanded && (
                    <ul className="dx-sp-col__sub">
                      {f.granularities.map((g) => {
                        const value = `${f.name}:${g}`
                        const sel = state.groupBy.includes(value)
                        return (
                          <li key={g}>
                            <button type="button" className="dx-sp-item" data-sp-nav aria-pressed={sel} onClick={() => onChange(toggleGroupBy(state, value))}>
                              <span className="dx-sp-item__check" aria-hidden="true">{sel && <Icon name="check" size={12} />}</span>
                              {L.granularity(g)}
                            </button>
                          </li>
                        )
                      })}
                    </ul>
                  )}
                </li>
              )
            }
            return (
              <li key={f.name}>
                <button
                  type="button"
                  className="dx-sp-item"
                  data-sp-nav
                  aria-pressed={on}
                  disabled={full}
                  onClick={() => onChange(toggleGroupBy(state, f.name))}
                >
                  <span className="dx-sp-item__check" aria-hidden="true">{on && <span className="dx-sp-item__lvl dx-num">{level + 1}</span>}</span>
                  {L.field(f.name)}
                </button>
              </li>
            )
          })}
        </ul>
        {state.groupBy.length >= MAX_GROUP_BY && <p className="dx-sp-col__empty">{t('search.painel.maxNiveis', { n: MAX_GROUP_BY })}</p>}
      </section>

      <FavoritesColumn id={id} favorites={favorites} />
    </div>
  )
}

function FavoritesColumn({ id, favorites }: { id: string; favorites: Favorites }) {
  const { t } = useTranslation()
  const [saving, setSaving] = useState(false)
  const [name, setName] = useState('')
  const [shared, setShared] = useState(false)
  const [isDefault, setIsDefault] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function save() {
    if (!name.trim()) return
    setBusy(true)
    setErr('')
    const e = await favorites.save({ name: name.trim(), shared, is_default: isDefault })
    setBusy(false)
    if (e) setErr(e)
    else {
      setSaving(false)
      setName('')
      setShared(false)
      setIsDefault(false)
    }
  }

  return (
    <section className="dx-sp-col" aria-labelledby={`${id}-s`}>
      <h3 id={`${id}-s`} className="dx-sp-col__title">
        <Icon name="star" size={13} />
        {t('search.painel.favoritos')}
      </h3>
      {!favorites.supported ? (
        <p className="dx-sp-col__empty">{t('search.favoritos.naoSuportado')}</p>
      ) : (
        <>
          {favorites.items.s === 'loading' && <Spinner label={t('search.estado.aCarregar')} />}
          {favorites.items.s === 'error' && <p className="dx-sp-col__err" role="alert">{favorites.items.msg}</p>}
          {favorites.items.s === 'ready' && (
            <ul className="dx-sp-col__list">
              {favorites.items.d.length === 0 && <li className="dx-sp-col__empty">{t('search.favoritos.vazio')}</li>}
              {favorites.items.d.map((f: SavedSearch) => (
                <li key={f.id} className="dx-sp-fav">
                  <button
                    type="button"
                    className="dx-sp-item"
                    data-sp-nav
                    disabled={!f.valid}
                    title={f.valid ? undefined : t('search.favoritos.invalido')}
                    onClick={() => favorites.apply(f)}
                  >
                    <span className="dx-sp-item__check" aria-hidden="true">{f.is_default && <Icon name="star" size={11} />}</span>
                    <span className="dx-sp-fav__name">{f.name}</span>
                    {f.shared && <Icon name="people" size={11} />}
                    {!f.editable && <span className="dx-muted dx-sp-fav__owner">{t('search.favoritos.deOutro', { nome: f.owner.username })}</span>}
                    {!f.valid && <span className="dx-muted">{t('search.favoritos.invalido')}</span>}
                  </button>
                  {f.editable && (
                    <button
                      type="button"
                      className="dx-sp-fav__del"
                      aria-label={t('search.favoritos.eliminar', { nome: f.name })}
                      title={t('search.favoritos.eliminar', { nome: f.name })}
                      onClick={async () => {
                        const e = await favorites.remove(f)
                        if (e) setErr(e)
                      }}
                    >
                      <Icon name="trash" size={12} />
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
          <div className="dx-sp-col__list has-sep">
            {!saving ? (
              <button type="button" className="dx-sp-item dx-sp-item--action" data-sp-nav aria-expanded={false} onClick={() => setSaving(true)}>
                <Icon name="plus" size={12} />
                {t('search.favoritos.guardarActual')}
              </button>
            ) : (
              <form
                className="dx-sp-save"
                onSubmit={(e) => {
                  e.preventDefault()
                  void save()
                }}
              >
                <TextInput
                  data-sp-nav
                  autoFocus
                  value={name}
                  maxLength={80}
                  aria-label={t('search.favoritos.nome')}
                  placeholder={t('search.favoritos.nome')}
                  onChange={(e) => setName(e.target.value)}
                />
                <Checkbox label={t('search.favoritos.predefinido')} checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)} />
                <Checkbox label={t('search.favoritos.partilhar')} checked={shared} onChange={(e) => setShared(e.target.checked)} />
                <Button type="submit" size="sm" variant="primary" busy={busy} disabled={!name.trim()}>
                  {t('search.favoritos.guardar')}
                </Button>
              </form>
            )}
            {err && (
              <p className="dx-sp-col__err" role="alert">
                {err}
              </p>
            )}
          </div>
        </>
      )}
    </section>
  )
}
