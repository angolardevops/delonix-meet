/**
 * Filiais e salas físicas da organização, em leitura. Qualquer membro as lê
 * (`require_member`); criá-las é da Administração (o «Gerir» vive na barra
 * do centro das Chamadas).
 */
import { useTranslation } from 'react-i18next'
import type { Branch, Employee, MeetingRoom } from '../../api'
import { Card, Empty } from '../../ui/kit'

export default function OrgOverview({
  branches,
  rooms,
  people,
}: {
  branches: Branch[]
  rooms: MeetingRoom[]
  people: Employee[]
}) {
  const { t } = useTranslation()
  const perBranch = (id: string) => people.filter((p) => p.branch_id === id).length
  return (
    <div className="org-overview">
      <Card title={t('org.filial.titulo')} eyebrow={String(branches.length)} flush>
        {branches.length === 0 ? (
          <Empty icon="building" title={t('org.filial.nenhuma')} />
        ) : (
          <ul className="org-simple">
            {branches.map((b) => (
              <li key={b.id}>
                <span className="org-simple__main">
                  <strong>{b.name}</strong>
                  <span className="dx-muted">{b.location || t('org.filial.semLocal')}</span>
                </span>
                <span className="dx-num dx-muted">{t('org.pessoasContagem', { count: perBranch(b.id) })}</span>
              </li>
            ))}
          </ul>
        )}
      </Card>
      <Card title={t('org.sala.titulo')} eyebrow={String(rooms.length)} flush>
        <p className="dx-muted org-card-note">{t('org.sala.dica')}</p>
        {rooms.length === 0 ? (
          <Empty icon="door" title={t('org.sala.nenhuma')} />
        ) : (
          <ul className="org-simple">
            {rooms.map((r) => (
              <li key={r.id}>
                <span className="org-simple__main">
                  <strong>{r.name}</strong>
                  <span className="dx-muted">{r.location || t('org.filial.semLocal')}</span>
                </span>
                <span className="dx-num dx-muted">{r.capacity ? t('org.sala.lugares', { count: r.capacity }) : '—'}</span>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  )
}
