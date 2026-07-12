import { ReactNode } from 'react'

// Estado vazio partilhado — ícone + título + dica + CTA opcional.
export default function EmptyState({
  icon,
  title,
  hint,
  action,
}: {
  icon?: ReactNode
  title: ReactNode
  hint?: ReactNode
  action?: ReactNode
}) {
  return (
    <div className="ds-empty" role="status">
      {icon && <div className="ds-empty-icon">{icon}</div>}
      <h3 className="ds-empty-title">{title}</h3>
      {hint && <p className="ds-empty-hint">{hint}</p>}
      {action && <div className="ds-empty-action">{action}</div>}
    </div>
  )
}
