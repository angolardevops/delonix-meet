/**
 * Quem esteve na sala da gravação (`…/participants`, página).
 *
 * Quem chega à gravação só por ela estar PUBLICADA leva `403
 * recording.participants_forbidden`: isso é uma recusa desta acção, escrita por
 * extenso no lugar da lista — não é sessão morta e não manda ninguém para o
 * login. Quem não vê a gravação nem chega aqui (leva `404`).
 */
import { useTranslation } from 'react-i18next'
import { isAbort, recordingParticipants } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Avatar } from '../../ui/kit'
import { recordingErrorMessage } from './apiErrors'
import { formatDateTimeShort } from './format'

export default function RecordingParticipants({ recordingId }: { recordingId: string }) {
  const { t, i18n } = useTranslation()
  const { state, reload } = useAsync(async () => {
    try {
      return await recordingParticipants(recordingId, { page_size: 100 })
    } catch (e) {
      if (isAbort(e)) throw e
      throw new Error(recordingErrorMessage(e, t, 'player.participantes.erro'))
    }
  }, [recordingId])
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
