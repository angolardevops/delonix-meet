import { ReactNode } from 'react'

// Cabeçalho de página partilhado — unifica o ritmo/estilo entre vistas.
// Reutiliza a classe .page-head existente e acrescenta uma linha de ações.
export default function PageHeader({
  icon,
  title,
  subtitle,
  actions,
}: {
  icon?: ReactNode
  title: ReactNode
  subtitle?: ReactNode
  actions?: ReactNode
}) {
  return (
    <header className="page-head ds-page-head">
      <div className="ds-page-head-main">
        <h1>
          {icon}
          {title}
        </h1>
        {subtitle && <p className="ds-page-sub">{subtitle}</p>}
      </div>
      {actions && <div className="ds-page-actions">{actions}</div>}
    </header>
  )
}
