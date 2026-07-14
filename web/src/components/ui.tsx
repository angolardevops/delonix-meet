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
} from 'react'

function cx(...parts: Array<string | false | undefined>): string {
  return parts.filter(Boolean).join(' ')
}

// ---------- Botões (Tier 1: ação · Tier 2: ícone quadrado) ----------

export type BtnVariant = 'primary' | 'ghost' | 'danger' | 'success' | 'link'

const BTN_CLASS: Record<BtnVariant, string> = {
  primary: 'btn-sm',
  ghost: 'btn-sm ghost',
  danger: 'btn-sm danger',
  success: 'btn-sm success',
  link: 'link small-link',
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
