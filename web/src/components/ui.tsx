/**
 * Kit de componentes do design system — wrappers FINOS sobre o sistema de
 * controlo único (styles.scss, camada "SISTEMA DE CONTROLO ÚNICO").
 *
 * Regras (ver docs/reference/design-system.md):
 *  · Botões novos usam <Btn>/<IconBtn> — nunca <button className="..."> solto.
 *  · Zero estilos inline de tamanho/raio — os tokens (--radius-*, --ctl-h)
 *    são a única fonte de verdade.
 *  · Variantes novas nascem AQUI + uma classe no CSS, nunca ad-hoc na página.
 */
import {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  useEffect,
  useRef,
  useState,
} from 'react'

function cx(...parts: Array<string | false | undefined>): string {
  return parts.filter(Boolean).join(' ')
}

// ---------- Botões (Tier 1: ação · Tier 2: ícone quadrado) ----------

export type BtnVariant = 'primary' | 'ghost' | 'danger' | 'success' | 'link' | 'submit'

const BTN_CLASS: Record<BtnVariant, string> = {
  primary: 'btn-sm',
  ghost: 'btn-sm ghost',
  danger: 'btn-sm danger',
  success: 'btn-sm success',
  link: 'link small-link',
  // CTA de formulário a toda a largura (login, lobby, modais) — já existia
  // como `button.primary` solto em várias páginas; formalizado aqui para
  // deixar de ser um className ad-hoc (docs/reference/design-system.md §3).
  submit: 'primary',
}

/** Botão de ação pequeno — altura/raio/tipografia únicos em toda a app. */
export function Btn({
  variant = 'primary',
  className,
  ...rest
}: { variant?: BtnVariant } & ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button {...rest} className={cx(BTN_CLASS[variant], className)} />
}

/** Botão-ícone quadrado 30×30 (fechar, recusar, ações de linha). */
export function IconBtn({
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button {...rest} className={cx('icon-btn', className)} />
}

// ---------- Superfícies (Tier 3) ----------

/** Cartão padrão (dash-card): superfície com raio médio único. */
export function Card({
  className,
  children,
  title,
  actions,
}: {
  className?: string
  children: ReactNode
  title?: ReactNode
  actions?: ReactNode
}) {
  return (
    <section className={cx('dash-card', className)}>
      {(title || actions) && (
        <header className="dash-card-head">
          {title && <h2>{title}</h2>}
          {actions}
        </header>
      )}
      {children}
    </section>
  )
}

// ---------- Formulários ----------

/** Rótulo + controlo empilhados (padrão dos painéis de definições). */
export function Field({
  label,
  hint,
  children,
}: {
  label: ReactNode
  hint?: ReactNode
  children: ReactNode
}) {
  return (
    <label className="set-label">
      {label}
      {children}
      {hint && <small className="muted">{hint}</small>}
    </label>
  )
}

/** Input de texto uniforme (o CSS global já lhe dá raio/altura). */
export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input type={props.type ?? 'text'} {...props} />
}

/** Select uniforme. */
export function SelectCtl(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} />
}

/** Switch iOS (o mesmo do lobby): input escondido + track desenhado. */
export function Switch({
  checked,
  onChange,
  disabled,
  ariaLabel,
}: {
  checked: boolean
  onChange: (next: boolean) => void
  disabled?: boolean
  ariaLabel?: string
}) {
  return (
    <span className="dx-switch">
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-label={ariaLabel}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="track" />
    </span>
  )
}

// ---------- Menu / SplitButton / Tabs ----------
// Generaliza o padrão já usado em .nav-account-menu (Shell) e .layout-menu
// (seletor de esquema da Sala): popover ancorado — o chamador tem de ser
// position:relative — que fecha ao clicar fora ou premir Esc. Nasce aqui
// para o Início e a Sala convergirem no mesmo menu em vez de cada um ter
// a sua versão (ver docs/reference/design-system.md).

/** Painel de popover ancorado. O elemento pai precisa de `position: relative`. */
export function Menu({
  open,
  onClose,
  align = 'start',
  side = 'bottom',
  className,
  children,
}: {
  open: boolean
  onClose: () => void
  align?: 'start' | 'end'
  side?: 'top' | 'bottom'
  className?: string
  children: ReactNode
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onEsc)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onEsc)
    }
  }, [open, onClose])
  if (!open) return null
  return (
    <div ref={ref} role="menu" className={cx('dx-menu', `dx-menu-${side}`, `dx-menu-${align}`, className)}>
      {children}
    </div>
  )
}

/** Item de menu — ícone opcional, etiqueta, dica curta por baixo. */
export function MenuItem({
  icon,
  label,
  hint,
  danger,
  active,
  disabled,
  onSelect,
}: {
  icon?: ReactNode
  label: ReactNode
  hint?: ReactNode
  danger?: boolean
  active?: boolean
  disabled?: boolean
  onSelect: () => void
}) {
  return (
    <button
      type="button"
      role="menuitem"
      className={cx('dx-menu-item', danger && 'dx-menu-item-danger', active && 'active')}
      disabled={disabled}
      onClick={onSelect}
    >
      {icon && <span className="dx-menu-item-icon">{icon}</span>}
      <span className="dx-menu-item-text">
        <strong>{label}</strong>
        {hint && <small>{hint}</small>}
      </span>
    </button>
  )
}

/** Separador fino entre grupos de itens. */
export function MenuDivider() {
  return <div className="dx-menu-sep" role="separator" />
}

/** Botão dividido: ação principal + seta que abre um {@link Menu}. Substitui
 *  grupos de chips soltas (ex.: "Nova reunião" + variantes na Home). */
export function SplitButton({
  label,
  icon,
  onClick,
  disabled,
  menuLabel,
  className,
  children,
}: {
  label: ReactNode
  icon?: ReactNode
  onClick: () => void
  disabled?: boolean
  /** Obrigatório e já traduzido pelo chamador — este kit não tem `useTranslation()`. */
  menuLabel: string
  className?: string
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  return (
    <div className={cx('dx-split', className)}>
      <button type="button" className="dx-split-main" onClick={onClick} disabled={disabled}>
        {icon}
        {label}
      </button>
      <button
        type="button"
        className="dx-split-caret"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={menuLabel}
        disabled={disabled}
        onClick={() => setOpen((o) => !o)}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round">
          <path d="m8 10 4 4 4-4" />
        </svg>
      </button>
      <Menu open={open} onClose={() => setOpen(false)} align="end" className="dx-split-menu">
        {children}
      </Menu>
    </div>
  )
}

/** Abas de painel (ex.: Conversa/Pessoas/Notas no painel lateral da Sala). */
export function Tabs<K extends string>({
  tabs,
  active,
  onChange,
}: {
  tabs: { key: K; label: ReactNode }[]
  active: K
  onChange: (k: K) => void
}) {
  return (
    <div className="dx-tabs" role="tablist">
      {tabs.map((t) => (
        <button
          key={t.key}
          type="button"
          role="tab"
          aria-selected={active === t.key}
          className={cx('dx-tab', active === t.key && 'active')}
          onClick={() => onChange(t.key)}
        >
          {t.label}
        </button>
      ))}
    </div>
  )
}
