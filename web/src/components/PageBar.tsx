/**
 * Barra de topo de uma página da consola (58 px): título, metadado em mono,
 * e as acções da página à direita. Em ecrã estreito mostra o botão que abre
 * a gaveta de navegação.
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { useShell } from './shellContext'

export default function PageBar({
  title,
  meta,
  children,
}: {
  title: ReactNode
  meta?: ReactNode
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const { navOpen, setNavOpen } = useShell()
  return (
    <header className="page-bar">
      <button
        type="button"
        className="dx-iconbtn page-bar__burger"
        aria-label={t('shell.abrirNavegacao')}
        aria-expanded={navOpen}
        aria-controls="shell-nav"
        onClick={() => setNavOpen(!navOpen)}
      >
        <Icon name="menu" />
      </button>
      <h1 className="page-bar__title">{title}</h1>
      {meta && <div className="page-bar__meta">{meta}</div>}
      <div className="dx-spacer" />
      {children && <div className="page-bar__actions">{children}</div>}
    </header>
  )
}
