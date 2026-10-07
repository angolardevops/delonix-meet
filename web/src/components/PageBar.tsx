/**
 * Barra de topo de uma página da consola (58 px): título, metadado em mono,
 * e as acções da página à direita. Em ecrã estreito mostra o botão que abre
 * a gaveta de navegação; em ecrã largo mostra-o só com o rail recolhido, e
 * aí expande-o.
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { parseHash, trilhoDe, type Degrau } from '../rota'
import { useShell } from './shellContext'

export default function PageBar({
  title,
  meta,
  trilho,
  children,
}: {
  title: ReactNode
  meta?: ReactNode
  /**
   * Os antecedentes da página, do Início até ao pai. A página actual NÃO entra:
   * é o `<h1>` ao lado. Vazio esconde o trilho — um trilho de um degrau não é
   * um trilho.
   *
   * Por omissão sai do endereço (`trilhoDe(parseHash())`), e é por isso que os
   * treze ecrãs com `PageBar` ganharam trilho sem se lhes tocar. Passa-se à mão
   * só onde o ecrã sabe mais do que o endereço.
   */
  trilho?: Degrau[]
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const { navExpanded, toggleNav } = useShell()
  const degraus = trilho ?? trilhoDe(parseHash())
  return (
    <header className="page-bar">
      <button
        type="button"
        className="dx-iconbtn page-bar__burger"
        aria-label={t('shell.abrirNavegacao')}
        title={`${t('shell.abrirNavegacao')} (${t('shell.atalhoMenu')})`}
        aria-expanded={navExpanded}
        aria-controls="shell-nav"
        onClick={toggleNav}
      >
        <Icon name="menu" />
      </button>
      {degraus.length > 0 && (
        <nav className="page-bar__trilho" aria-label={t('shell.trilho')}>
          {degraus.map((d) => (
            <a key={d.hash} href={d.hash}>
              {t(d.chave)}
            </a>
          ))}
        </nav>
      )}
      <h1 className="page-bar__title">{title}</h1>
      {meta && <div className="page-bar__meta">{meta}</div>}
      <div className="dx-spacer" />
      {children && <div className="page-bar__actions">{children}</div>}
    </header>
  )
}
