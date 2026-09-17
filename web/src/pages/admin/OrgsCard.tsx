/**
 * Organizações de quem administra — a tabela do template («Organizações e
 * licenças») só com as colunas que GET /api/orgs devolve: nome, domínio,
 * papel, pessoas e retenção. Região, lugares contratados, plano, renovação e
 * estado de licença não existem no servidor e por isso não têm coluna.
 *
 * Pesquisa: o servidor não descreve organizações (nem na fase 2 do contrato);
 * a lista que GET /api/orgs devolve é inteira e o painel filtra-a aqui.
 */
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type { OrgSummary } from '../../api'
import { Button, Card, cx, StatusBadge } from '../../ui/kit'
import { SearchBar, SearchResults } from '../../ui/search/SearchResults'
import { useResourceSearch } from '../../ui/search/useResourceSearch'
import { orgsFallback } from './search'

export default function OrgsCard({
  orgs,
  activeId,
  onSelect,
}: {
  orgs: OrgSummary[]
  activeId: string
  onSelect: (id: string) => void
}) {
  const { t } = useTranslation()
  const fallback = useMemo(() => orgsFallback(orgs), [orgs])
  const rs = useResourceSearch<OrgSummary>({ resource: null, ns: 'orgs.', fallback, deps: [orgs] })
  return (
    <Card
      title={t('consola.orgs.titulo')}
      actions={
        <span className="dx-num dx-muted org-meta">
          {t('consola.orgs.contagem', { count: orgs.length })} · {t('consola.orgs.pessoasTotal', { count: orgs.reduce((n, o) => n + o.member_count, 0) })}
        </span>
      }
      flush
      className="org-boxed"
    >
      <div className="org-card-pad org-search-pad">
        <SearchBar rs={rs} label={t('search.rotulos.orgs')} />
      </div>
      <SearchResults
        rs={rs}
        emptyIcon="building"
        emptyTitle={t('org.semOrg.titulo')}
        renderItems={(rows) => (
      <div className="dx-table-wrap org-table-wrap">
        <table className="dx-table org-table" data-testid="admin-orgs">
          <thead>
            <tr>
              <th scope="col">{t('consola.orgs.organizacao')}</th>
              <th scope="col">{t('consola.orgs.papel')}</th>
              <th scope="col">{t('consola.orgs.pessoas')}</th>
              <th scope="col">{t('consola.orgs.retencao')}</th>
              <th scope="col">
                <span className="dx-sr-only">{t('consola.orgs.accoes')}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((o) => (
              <tr key={o.id} className={cx(o.id === activeId && 'org-row--active')} aria-current={o.id === activeId ? 'true' : undefined}>
                <td>
                  <span className="org-person__text">
                    <strong>{o.name}</strong>
                    <span className="dx-muted dx-num">{o.domain || o.slug}</span>
                  </span>
                </td>
                <td>
                  <StatusBadge tone={o.role === 'admin' ? 'success' : 'neutral'}>
                    {o.role === 'admin' ? t('org.papel.admin') : t('org.papel.membro')}
                  </StatusBadge>
                </td>
                <td className="dx-num">{o.member_count}</td>
                <td className="dx-num dx-muted">
                  {(o.retention_days ?? 0) > 0 ? t('consola.orgs.dias', { count: o.retention_days }) : t('consola.orgs.semLimite')}
                </td>
                <td className="org-nowrap">
                  {o.id !== activeId && (
                    <Button size="sm" variant="ghost" onClick={() => onSelect(o.id)}>
                      {t('consola.orgs.abrir')}
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
        )}
      />
    </Card>
  )
}
