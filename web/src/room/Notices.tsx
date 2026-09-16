import { useTranslation } from 'react-i18next'
import type { PeerInfo, PollView } from '../signaling'
import { Icon } from '../ui/icons'
import { Avatar, Button, IconButton } from '../ui/kit'
import { PollCard } from './PollCard'

/**
 * Avisos da reunião. REGIÃO VIVA (R104): estes cartões pedem uma DECISÃO —
 * alguém à porta, um pedido de partilha, uma sondagem — e `assertive` porque
 * interrompem de propósito.
 */
export function Notices({
  canAdmit,
  waitingQueue,
  onAdmit,
  onAdmitAll,
  ctrlAsk,
  onAnswerCtrl,
  shareAsk,
  isHost,
  onAnswerShare,
  poll,
  myVote,
  onVote,
  onDismissPoll,
  companion,
  onUseAudioHere,
  recNotice,
  talkOverNames,
}: {
  canAdmit: boolean
  waitingQueue: PeerInfo[]
  onAdmit: (peerId: string, ok: boolean) => void
  onAdmitAll: () => void
  ctrlAsk: { from: string; username: string } | null
  onAnswerCtrl: (accept: boolean) => void
  shareAsk: { from: string; username: string } | null
  isHost: boolean
  onAnswerShare: (allowed: boolean) => void
  poll: PollView | null
  myVote: number | undefined
  onVote: (pollId: string, option: number) => void
  onDismissPoll: (pollId: string) => void
  companion: boolean
  onUseAudioHere: () => void
  recNotice: string
  /** `null` sem fala simultânea; texto vazio quando não há nomes. */
  talkOverNames: string | null
}) {
  const { t } = useTranslation()
  return (
    <div className="rm-notices" role="region" aria-live="assertive" aria-label={t('room.avisos.regiao')}>
      {canAdmit && waitingQueue.length > 0 && (
        <section className="rm-notice" aria-label={t('room.avisos.salaDeEspera')}>
          <header className="rm-notice__head">
            <Icon name="door" size={13} />
            <strong>{t('room.avisos.salaDeEspera')}</strong>
            <span className="dx-spacer" />
            {waitingQueue.length > 1 && (
              <button type="button" className="rm-link" onClick={onAdmitAll}>
                {t('room.avisos.admitirTodos', { count: waitingQueue.length })}
              </button>
            )}
          </header>
          {waitingQueue.map((p) => (
            <div key={p.peer_id} className="rm-notice__row">
              <Avatar name={p.username} size={26} />
              <span className="rm-notice__who">
                <strong>{p.username}</strong>
                <small className="dx-muted">{t('room.avisos.querEntrar')}</small>
              </span>
              <Button size="sm" variant="outline" className="rm-admit-deny" onClick={() => onAdmit(p.peer_id, false)}>
                {t('room.avisos.recusar')}
              </Button>
              <Button size="sm" variant="primary" className="rm-admit-accept" onClick={() => onAdmit(p.peer_id, true)}>
                {t('room.avisos.admitir')}
              </Button>
            </div>
          ))}
        </section>
      )}

      {ctrlAsk && (
        <section className="rm-notice" aria-label={t('room.avisos.pedidoControlo')}>
          <div className="rm-notice__row">
            <Icon name="cube" />
            <span className="rm-notice__who">
              <strong>{ctrlAsk.username}</strong>
              <small className="dx-muted">{t('room.avisos.pedeControlo')}</small>
            </span>
            <Button size="sm" variant="outline" onClick={() => onAnswerCtrl(false)}>
              {t('room.avisos.recusar')}
            </Button>
            <Button size="sm" variant="primary" onClick={() => onAnswerCtrl(true)}>
              {t('room.avisos.aceitar')}
            </Button>
          </div>
        </section>
      )}

      {shareAsk && isHost && (
        <section className="rm-notice" aria-label={t('room.avisos.pedidoPartilha')}>
          <div className="rm-notice__row">
            <Icon name="screen" />
            <span className="rm-notice__who">
              <strong>{shareAsk.username}</strong>
              <small className="dx-muted">{t('room.avisos.querPartilhar')}</small>
            </span>
            <Button size="sm" variant="outline" onClick={() => onAnswerShare(false)}>
              {t('room.avisos.negar')}
            </Button>
            <Button size="sm" variant="primary" onClick={() => onAnswerShare(true)}>
              {t('room.avisos.permitir')}
            </Button>
          </div>
        </section>
      )}

      {poll && (
        <section className="rm-notice rm-notice--poll" aria-label={t('room.sondagens.sondagem')}>
          <div className="rm-notice__close">
            <IconButton icon="x" bare label={t('room.avisos.dispensar')} onClick={() => onDismissPoll(poll.id)} />
          </div>
          <PollCard poll={poll} myVote={myVote} isHost={false} onVote={(i) => onVote(poll.id, i)} compact />
        </section>
      )}

      {companion && (
        <section className="rm-notice rm-notice--companion" role="status">
          <div className="rm-notice__row">
            <Icon name="volume" />
            <span className="rm-notice__who">
              <strong>{t('room.companion.tituloSemAudio')}</strong>
              <small>{t('room.companion.explicacao')}</small>
            </span>
            <Button size="sm" variant="primary" onClick={onUseAudioHere}>
              {t('room.companion.usarAudioAqui')}
            </Button>
          </div>
        </section>
      )}

      {recNotice && (
        <div className="rm-toast rm-toast--rec" role="status">
          <span className="rm-toast__dot" aria-hidden="true" />
          <span>
            <strong>{recNotice}</strong> {t('room.gravacao.todosAvisados')}
          </span>
        </div>
      )}

      {talkOverNames !== null && (
        <div className="rm-toast" role="status">
          <Icon name="mic" size={13} />
          <span>{talkOverNames ? t('room.avisos.falamAoMesmoTempo', { nomes: talkOverNames }) : t('room.avisos.duasPessoasAFalar')}</span>
        </div>
      )}
    </div>
  )
}
