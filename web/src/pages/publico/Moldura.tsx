/**
 * Moldura das páginas públicas (estado, API, termos, partilha): sem rail nem
 * sessão, só a marca, as três páginas públicas e o regresso à entrada. Não
 * importa o Shell — estas páginas abrem-se sem conta.
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { BrandLockup } from '../../components/BrandMark'
import { cx } from '../../ui/kit'
import '../../ui/publico.css'

export type PaginaPublica = 'status' | 'api-docs' | 'legal' | 'share' | 'invite'

const LIGACOES: { k: Exclude<PaginaPublica, 'share' | 'invite'>; chave: string }[] = [
  { k: 'status', chave: 'publico.comum.estado' },
  { k: 'api-docs', chave: 'publico.comum.api' },
  { k: 'legal', chave: 'publico.comum.legal' },
]

export default function Moldura({ pagina, children, estreita }: { pagina: PaginaPublica; children: ReactNode; estreita?: boolean }) {
  const { t } = useTranslation()
  return (
    <div className="pub">
      <header className="pub-topo">
        <a href="#/" className="pub-topo__marca">
          <BrandLockup size={24} tone="tile" />
        </a>
        <nav className="pub-topo__nav" aria-label={t('publico.comum.navegacao')}>
          {LIGACOES.map((l) => (
            <a key={l.k} href={`#/${l.k}`} aria-current={pagina === l.k ? 'page' : undefined}>
              {t(l.chave)}
            </a>
          ))}
        </nav>
        <a href="#/" className="pub-topo__voltar dx-btn dx-btn--secondary dx-btn--sm">
          {t('publico.comum.voltar')}
        </a>
      </header>
      <main className={cx('pub-corpo', estreita && 'pub-corpo--estreito')}>{children}</main>
    </div>
  )
}
