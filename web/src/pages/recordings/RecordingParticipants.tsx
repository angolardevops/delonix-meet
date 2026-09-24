/** Quem esteve na sala da gravação (`…/participants`, página). */
import { useTranslation } from 'react-i18next'
import { recordingParticipants } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Avatar } from '../../ui/kit'
import { formatDateTimeShort } from './format'

export default function RecordingParticipants({ recordingId }: { recordingId: string }) {
  const { t, i18n } = useTranslation()
  const { state, reload } = useAsync(() => recordingParticipants(recordingId, { page_size: 100 }), [recordingId])
  return (
    <AsyncSection state={state} onRetry={reload}>
      {(page) =>
        page.items.length === 0 ? (
          <p className="pl-empty">{t('player.participantes.vazio')}</p>
        ) : (
          <ul className="pl-people">
            {page.items.map((p) => (
              <li key={p.user_id}>
                <Avatar name={p.username} size={24} />
                <span className="pl-people__name">{p.username}</span>
                <span className="dx-muted dx-num">{t('player.participantes.entrou', { when: formatDateTimeShort(p.joined_at, i18n.language) })}</span>
              </li>
            ))}
          </ul>
        )
      }
    </AsyncSection>
  )
}
