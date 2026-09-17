import { useTranslation } from 'react-i18next'
import type { QaView } from '../signaling'
import { Icon } from '../ui/icons'
import { cx } from '../ui/kit'
import type { MeetingTools } from './useMeetingTools'

/**
 * Uma pergunta do Q&A como cartão (template DelonixRoomChat, «PERGUNTA · POR
 * MODERAR»): estado, texto, autor e votos, e — para o anfitrião — destacar no
 * palco e ocultar. O servidor decide quem recebe as ocultas; aqui só se mostra.
 */
export function QuestionCard({ q, tools, isHost }: { q: QaView; tools: MeetingTools; isHost: boolean }) {
  const { t } = useTranslation()
  const estado = q.hidden
    ? t('room.perguntas.oculta')
    : q.spotlight
      ? t('room.perguntas.noPalco')
      : q.answered
        ? t('room.perguntas.respondida')
        : isHost
          ? t('room.perguntas.porModerar')
          : null
  return (
    <article className={cx('rm-qcard', q.spotlight && 'is-spot', q.hidden && 'is-hidden', q.answered && 'is-answered')}>
      <div className="rm-qcard__head">
        {estado ? t('room.perguntas.etiquetaEstado', { estado }) : t('room.perguntas.etiqueta')}
      </div>
      <p className="rm-qcard__text">{t('room.perguntas.citacao', { texto: q.text })}</p>
      {q.hidden && <p className="rm-qcard__note">{t('room.perguntas.ocultaDica')}</p>}
      <div className="rm-qcard__foot">
        <button
          type="button"
          className={cx('rm-qcard__vote', tools.myUpvotes[q.id] && 'is-on')}
          aria-pressed={!!tools.myUpvotes[q.id]}
          aria-label={t('room.perguntas.votar', { count: q.upvotes })}
          title={t('room.perguntas.votar', { count: q.upvotes })}
          onClick={() => tools.upvote(q.id)}
        >
          {t('room.perguntas.autorVotos', { nome: q.by, n: q.upvotes })}
        </button>
        <span className="dx-spacer" />
        {isHost && (
          <>
            {!q.hidden && (
              <button
                type="button"
                className={cx('rm-qcard__btn', !q.spotlight && 'is-primary')}
                onClick={() => tools.spotlightQuestion(q.spotlight ? null : q.id)}
              >
                {q.spotlight ? t('room.perguntas.tirarDoPalco') : t('room.perguntas.destacar')}
              </button>
            )}
            <button type="button" className="rm-qcard__btn" onClick={() => tools.hideQuestion(q.id, !q.hidden)}>
              {q.hidden ? t('room.perguntas.mostrar') : t('room.perguntas.ocultar')}
            </button>
            <button
              type="button"
              className="rm-qcard__btn is-icon"
              aria-label={q.answered ? t('room.perguntas.reabrir') : t('room.perguntas.marcarRespondida')}
              title={q.answered ? t('room.perguntas.reabrir') : t('room.perguntas.marcarRespondida')}
              onClick={() => tools.markAnswered(q.id)}
            >
              <Icon name={q.answered ? 'undo' : 'check'} size={11} />
            </button>
          </>
        )}
      </div>
    </article>
  )
}
