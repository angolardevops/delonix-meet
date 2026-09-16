/**
 * Kit de componentes do Delonix Meet. Uma página compõe-se daqui: botão,
 * campo, cartão, badge, tabela, diálogo. Variante nova = classe em
 * `base.css` + propriedade aqui, nunca estilo escrito na página.
 */
import {
  ButtonHTMLAttributes,
  forwardRef,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
  useEffect,
  useId,
  useRef,
} from 'react'
import { useTranslation } from 'react-i18next'
import { Icon, IconName } from './icons'

function cx(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(' ')
}
export { cx }

type BtnVariant = 'primary' | 'secondary' | 'ghost' | 'outline' | 'danger' | 'live'

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & {
    variant?: BtnVariant
    size?: 'sm' | 'md' | 'lg'
    block?: boolean
    icon?: IconName
    busy?: boolean
  }
>(function Button({ variant = 'secondary', size = 'md', block, icon, busy, className, children, type, disabled, ...rest }, ref) {
  return (
    <button
      ref={ref}
      type={type ?? 'button'}
      className={cx('dx-btn', `dx-btn--${variant}`, size !== 'md' && `dx-btn--${size}`, block && 'dx-btn--block', className)}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      {busy ? <span className="dx-spinner" aria-hidden="true" /> : icon ? <Icon name={icon} /> : null}
      {children}
    </button>
  )
})

export const IconButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string; bare?: boolean }
>(function IconButton({ icon, label, bare, className, type, ...rest }, ref) {
  return (
    <button
      ref={ref}
      type={type ?? 'button'}
      className={cx('dx-iconbtn', bare && 'dx-iconbtn--bare', className)}
      aria-label={label}
      title={label}
      {...rest}
    >
      <Icon name={icon} />
    </button>
  )
})

export function Card({
  title,
  actions,
  eyebrow,
  children,
  className,
  bodyClass,
  flush,
  as: Tag = 'section',
}: {
  title?: ReactNode
  actions?: ReactNode
  eyebrow?: ReactNode
  children?: ReactNode
  className?: string
  bodyClass?: string
  flush?: boolean
  as?: 'section' | 'div' | 'article'
}) {
  return (
    <Tag className={cx('dx-card', flush && 'dx-card--flush', className)}>
      {(title || actions) && (
        <header className="dx-card__head">
          {title && <h2 className="dx-card__title" style={{ margin: 0 }}>{title}</h2>}
          {eyebrow && <span className="dx-muted dx-num" style={{ fontSize: 10.5 }}>{eyebrow}</span>}
          <span className="dx-spacer" />
          {actions}
        </header>
      )}
      <div className={cx('dx-card__body', bodyClass)}>{children}</div>
    </Tag>
  )
}

export function SectionHead({ title, meta, action }: { title: ReactNode; meta?: ReactNode; action?: ReactNode }) {
  return (
    <div className="dx-section-head">
      <h2>{title}</h2>
      {meta}
      <span className="dx-spacer" />
      {action}
    </div>
  )
}

export function Tag({ tone, children, plain }: { tone?: 'live' | 'accent' | 'success'; children: ReactNode; plain?: boolean }) {
  return <span className={cx('dx-tag', tone && `dx-tag--${tone}`, plain && 'dx-tag--plain')}>{children}</span>
}

export type BadgeTone = 'record' | 'live' | 'warning' | 'success' | 'neutral'

/** Badge de estado: cor + forma (ponto, triângulo, ícone) + texto. */
export function StatusBadge({ tone, children, icon }: { tone: BadgeTone; children: ReactNode; icon?: IconName }) {
  const mark =
    tone === 'live' ? (
      <span className="dx-badge__tri" aria-hidden="true" />
    ) : icon ? (
      <Icon name={icon} size={11} />
    ) : (
      <span className="dx-badge__dot" aria-hidden="true" />
    )
  return (
    <span className={cx('dx-badge', `dx-badge--${tone}`)}>
      {mark}
      {children}
    </span>
  )
}

export function Field({
  label,
  hint,
  error,
  aside,
  children,
  htmlFor,
}: {
  label: ReactNode
  hint?: ReactNode
  error?: ReactNode
  aside?: ReactNode
  children: ReactNode
  htmlFor?: string
}) {
  return (
    <div className="dx-field">
      <label className="dx-field__label" htmlFor={htmlFor}>
        <span>{label}</span>
        {aside}
      </label>
      {children}
      {error ? (
        <span className="dx-field__error" role="alert">
          {error}
        </span>
      ) : hint ? (
        <span className="dx-field__hint">{hint}</span>
      ) : null}
    </div>
  )
}

export const TextInput = forwardRef<
  HTMLInputElement,
  InputHTMLAttributes<HTMLInputElement> & { large?: boolean; code?: boolean }
>(function TextInput({ large, code, className, ...rest }, ref) {
  return <input ref={ref} className={cx('dx-input', large && 'dx-input--lg', code && 'dx-input--code', className)} {...rest} />
})

export const TextArea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(function TextArea(
  { className, ...rest },
  ref,
) {
  return <textarea ref={ref} className={cx('dx-textarea', className)} {...rest} />
})

export function Select({ className, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select className={cx('dx-select', className)} {...rest}>
      {children}
    </select>
  )
}

export function Checkbox({
  label,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & { label: ReactNode }) {
  return (
    <label className="dx-check">
      <input type="checkbox" {...rest} />
      <span>{label}</span>
    </label>
  )
}

export function Toggle({
  label,
  hint,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> & { label: ReactNode; hint?: ReactNode }) {
  return (
    <label className="dx-toggle">
      <input type="checkbox" role="switch" {...rest} />
      <span>
        <span style={{ display: 'block', fontWeight: 600 }}>{label}</span>
        {hint && <span className="dx-muted" style={{ fontSize: 10.5 }}>{hint}</span>}
      </span>
    </label>
  )
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T
  options: { value: T; label: ReactNode }[]
  onChange: (v: T) => void
  label: string
}) {
  return (
    <div className="dx-seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  )
}

export function Tabs<T extends string>({
  value,
  tabs,
  onChange,
  label,
}: {
  value: T
  tabs: { value: T; label: ReactNode; count?: number }[]
  onChange: (v: T) => void
  label: string
}) {
  return (
    <div className="dx-tabs" role="tablist" aria-label={label}>
      {tabs.map((tab) => (
        <button
          key={tab.value}
          type="button"
          role="tab"
          aria-selected={tab.value === value}
          onClick={() => onChange(tab.value)}
        >
          {tab.label}
          {tab.count ? <span className="dx-count">{tab.count}</span> : null}
        </button>
      ))}
    </div>
  )
}

/** Cor estável por nome — o mesmo participante tem sempre o mesmo tom. */
const AVATAR_TONES = ['#9e2026', '#3c5a7a', '#7a4a68', '#3d6a5e', '#6b5a2e', '#47617d', '#3a6b73', '#6a3d3d', '#4f4a7a', '#5a6b3d']

export function avatarTone(seed: string) {
  let h = 0
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0
  return AVATAR_TONES[h % AVATAR_TONES.length]
}

export function initials(name: string) {
  const parts = name.trim().split(/[\s._@-]+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

export function Avatar({ name, size = 24 }: { name: string; size?: number }) {
  return (
    <span
      className="dx-avatar"
      style={{ width: size, height: size, fontSize: Math.max(9, Math.round(size * 0.38)), background: avatarTone(name) }}
      aria-hidden="true"
    >
      {initials(name)}
    </span>
  )
}

export function AvatarStack({ names, max = 3, size = 24 }: { names: string[]; max?: number; size?: number }) {
  const shown = names.slice(0, max)
  const extra = names.length - shown.length
  return (
    <span className="dx-avatars" title={names.join(', ')}>
      {shown.map((n, i) => (
        <Avatar key={n + i} name={n} size={size} />
      ))}
      {extra > 0 && (
        <span className="dx-avatar dx-num" style={{ width: size, height: size, fontSize: 9, background: 'var(--border)', color: 'var(--muted)' }}>
          +{extra}
        </span>
      )}
    </span>
  )
}

export function Meter({ value, tone }: { value: number; tone?: 'success' | 'live' }) {
  const v = Math.max(0, Math.min(100, value))
  return (
    <div className={cx('dx-meter', tone && `dx-meter--${tone}`)} role="presentation">
      <span style={{ width: `${v}%` }} />
    </div>
  )
}

export function Empty({ icon = 'info', title, children, action }: { icon?: IconName; title: ReactNode; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="dx-empty">
      <Icon name={icon} />
      <div className="dx-empty__title">{title}</div>
      {children && <div style={{ maxWidth: 420 }}>{children}</div>}
      {action}
    </div>
  )
}

export function Alert({ tone, children, icon }: { tone?: 'danger' | 'warning' | 'success'; children: ReactNode; icon?: IconName }) {
  return (
    <div className={cx('dx-alert', tone && `dx-alert--${tone}`)} role={tone === 'danger' ? 'alert' : 'status'}>
      <Icon name={icon ?? (tone === 'success' ? 'check' : tone ? 'alert' : 'info')} />
      <div style={{ minWidth: 0 }}>{children}</div>
    </div>
  )
}

export function Spinner({ label }: { label?: string }) {
  return <span className="dx-spinner" role="status" aria-label={label} />
}

export function Skeleton({ h = 14, w = '100%' }: { h?: number; w?: number | string }) {
  return <div className="dx-skeleton" style={{ height: h, width: w }} aria-hidden="true" />
}

/**
 * Diálogo modal: foco preso ao abrir, Esc fecha, o foco volta a quem abriu.
 */
export function Dialog({
  title,
  onClose,
  children,
  footer,
  wide,
}: {
  title: ReactNode
  onClose: () => void
  children: ReactNode
  footer?: ReactNode
  wide?: boolean
}) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const titleId = useId()
  // O fecho vive numa ref: um `onClose` novo a cada render do pai não pode
  // voltar a correr o efeito (roubava o foco ao campo onde se está a escrever).
  const closeRef = useRef(onClose)
  closeRef.current = onClose
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null
    const el = ref.current
    const first = el?.querySelector<HTMLElement>('input, select, textarea, button:not([data-close])')
    ;(first ?? el)?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        closeRef.current()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      prev?.focus?.()
    }
  }, [])
  return (
    <div className="dx-dialog-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={ref}
        className={cx('dx-dialog', wide && 'dx-dialog--wide')}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header className="dx-dialog__head">
          <h2 id={titleId}>{title}</h2>
          <IconButton icon="x" label={t('ui.fechar')} bare onClick={onClose} data-close />
        </header>
        <div className="dx-dialog__body">{children}</div>
        {footer && <footer className="dx-dialog__foot">{footer}</footer>}
      </div>
    </div>
  )
}
