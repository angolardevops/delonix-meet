/**
 * Detalhe do contacto (pessoa ou grupo) no lugar onde o template tem o vídeo:
 * a chamada acontece na sala, aqui só se escolhe como ligar.
 */
import { useTranslation } from 'react-i18next'
import type { Employee, Group } from '../../api'
import { Icon } from '../../ui/icons'
import { Avatar, Button, StatusBadge, Tag } from '../../ui/kit'
import { formatAgo, useLocaleTag } from '../admin/orgShared'

export function PersonDetail({
  person,
  me,
  online,
  onCall,
}: {
  person: Employee
  me: boolean
  online: boolean
  onCall: (kind: 'video' | 'voice') => void
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const ago = formatAgo(person.last_active, locale)
  return (
    <div className="org-detail">
      <div className="org-detail__hero">
        <span className="org-av org-av--xl">
          <Avatar name={person.username} size={88} />
          <span className={online ? 'org-dot org-dot--on' : 'org-dot'} aria-hidden="true" />
        </span>
        <h2 className="org-detail__name">{person.username}</h2>
        <div className="org-detail__tags">
          <StatusBadge tone={online ? 'success' : 'neutral'}>
            {online ? t('org.presenca.online') : t('org.presenca.offline')}
          </StatusBadge>
          {person.role === 'admin' && <Tag tone="accent">{t('org.papel.admin')}</Tag>}
          {me && <Tag plain>{t('org.dir.tuMesmo')}</Tag>}
        </div>
        {!me && (
          <div className="org-detail__actions">
            <Button variant="primary" size="lg" icon="video" onClick={() => onCall('video')}>
              {t('org.dir.videochamada')}
            </Button>
            <Button variant="secondary" size="lg" icon="phone" onClick={() => onCall('voice')}>
              {t('org.dir.chamadaVoz')}
            </Button>
          </div>
        )}
        {!me && !online && <p className="dx-muted org-detail__note">{t('org.dir.offlineNota')}</p>}
      </div>
      <dl className="dx-kv org-detail__kv">
        <dt>{t('org.campo.cargo')}</dt>
        <dd>{person.title || '—'}</dd>
        <dt>{t('org.campo.filial')}</dt>
        <dd>{person.branch_name || '—'}</dd>
        <dt>{t('org.campo.email')}</dt>
        <dd className="dx-num org-break">{person.email}</dd>
        <dt>{t('org.campo.ultimaActividade')}</dt>
        <dd className="dx-num">{ago ?? '—'}</dd>
      </dl>
    </div>
  )
}

export function GroupDetail({ group, onCall }: { group: Group; onCall: (kind: 'video' | 'voice') => void }) {
  const { t } = useTranslation()
  return (
    <div className="org-detail">
      <div className="org-detail__hero">
        <span className="org-av org-av--xl org-av--group" aria-hidden="true">
          <Icon name="people" />
        </span>
        <h2 className="org-detail__name">{group.name}</h2>
        <div className="org-detail__tags">
          <Tag plain>{t('org.membrosContagem', { count: group.member_count })}</Tag>
        </div>
        <div className="org-detail__actions">
          <Button variant="primary" size="lg" icon="video" onClick={() => onCall('video')}>
            {t('org.dir.videochamadaGrupo')}
          </Button>
          <Button variant="secondary" size="lg" icon="phone" onClick={() => onCall('voice')}>
            {t('org.dir.chamadaVozGrupo')}
          </Button>
        </div>
        <p className="dx-muted org-detail__note">{t('org.dir.grupoNota')}</p>
      </div>
    </div>
  )
}
