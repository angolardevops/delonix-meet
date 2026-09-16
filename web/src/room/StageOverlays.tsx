import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import { Icon } from '../ui/icons'
import { Button, IconButton, Spinner } from '../ui/kit'
import type { FloatingReaction } from './useReactions'

export function ReactionsLayer({ reactions }: { reactions: FloatingReaction[] }) {
  const { t } = useTranslation()
  return (
    <div className="rm-reactions" aria-hidden="true">
      {reactions.map((r, i) => (
        <span key={r.id} className="rm-reaction" style={{ left: `${12 + ((r.id * 37 + i * 11) % 60)}%` }}>
          <span className="rm-reaction__emoji">{r.emoji}</span>
          <span className="rm-reaction__who">{r.own ? t('room.tile.tu') : r.username}</span>
        </span>
      ))}
    </div>
  )
}

export function CaptionOverlay({ caption }: { caption: { who: string; text: string } }) {
  return (
    <div className="rm-caption" aria-live="polite">
      <strong>{caption.who}</strong> {caption.text}
    </div>
  )
}

/** A festa de quem acerta no quiz (~4,5 s). */
export function WinnerOverlay() {
  const { t } = useTranslation()
  return (
    <div className="rm-winner" aria-hidden="true">
      {Array.from({ length: 14 }, (_, i) => (
        <span key={i} className={`rm-winner__spark s${i % 7}`} style={{ animationDelay: `${(i % 5) * 0.25}s` }} />
      ))}
      <div className="rm-winner__card">
        <Icon name="trophy" size={30} />
        <strong>{t('room.sondagens.acertaste')}</strong>
      </div>
    </div>
  )
}

export function WaitingOverlay() {
  const { t } = useTranslation()
  return (
    <div className="rm-waiting" role="status">
      <Spinner />
      <h2>{t('room.espera.titulo')}</h2>
      <p className="dx-muted">{t('room.espera.texto')}</p>
    </div>
  )
}

/**
 * Cartão «a tua reunião está pronta» — SÓ para o anfitrião: um convidado não é
 * dono da reunião e não deve ser convidado a partilhar o link como se fosse.
 */
export function ReadyCard({
  code,
  waitingRoomOn,
  onAddPeople,
  onDismiss,
}: {
  code: string
  waitingRoomOn: boolean
  onAddPeople: () => void
  onDismiss: () => void
}) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const link = `${location.origin}/#/r/${code}`
  return (
    <section className="rm-ready" aria-labelledby="rm-ready-title">
      <header className="rm-ready__head">
        <h2 id="rm-ready-title">{t('room.pronta.titulo')}</h2>
        <IconButton icon="x" bare label={t('room.pronta.dispensar')} onClick={onDismiss} />
      </header>
      <Button variant="primary" size="sm" icon="userPlus" onClick={onAddPeople}>
        {t('room.pronta.adicionar')}
      </Button>
      <p className="dx-muted">{t('room.pronta.ouPartilha')}</p>
      <div className="rm-ready__link">
        <span className="dx-num">{link.replace(/^https?:\/\//, '')}</span>
        <IconButton
          icon={copied ? 'check' : 'copy'}
          label={copied ? t('room.pronta.copiado') : t('room.pronta.copiar')}
          onClick={() => {
            void navigator.clipboard?.writeText(link).then(() => {
              setCopied(true)
              setTimeout(() => setCopied(false), 1500)
            })
          }}
        />
      </div>
      <p className="dx-muted rm-ready__note">
        <Icon name="shield" size={12} />
        {waitingRoomOn ? t('room.pronta.pedeAutorizacao') : t('room.pronta.entraDirecto')}
      </p>
      <p className="dx-muted">{t('room.pronta.aParticiparComo', { nome: currentUser()?.username ?? '' })}</p>
    </section>
  )
}
