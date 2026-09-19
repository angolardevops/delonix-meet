import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Avatar, Button, Dialog, Field, IconButton, TextInput } from '../ui/kit'
import type { Invite } from './useInvite'

/** Chamar membros da organização para esta sala. */
export function InviteDialog({ invite }: { invite: Invite }) {
  const { t } = useTranslation()
  return (
    <Dialog
      title={t('room.convite.titulo')}
      onClose={invite.close}
      footer={
        <>
          <Button variant="ghost" onClick={invite.close}>
            {t('room.convite.fechar')}
          </Button>
          <Button variant="primary" icon="phone" busy={invite.busy} disabled={invite.selected.length === 0} onClick={() => void invite.send()}>
            {t('room.convite.chamar', { count: invite.selected.length })}
          </Button>
        </>
      }
    >
      <Field label={t('room.convite.pesquisar')} htmlFor="rm-invite-q" hint={t('room.convite.pesquisarDica')}>
        <TextInput id="rm-invite-q" value={invite.query} autoComplete="off" onChange={(e) => invite.setQuery(e.target.value)} />
      </Field>
      {invite.results.length > 0 && (
        <ul className="rm-invite__list">
          {invite.results.map((u) => (
            <li key={u.id}>
              <button type="button" className="rm-invite__item" onClick={() => invite.select(u)}>
                <Avatar name={u.username} size={26} />
                <span className="rm-invite__who">
                  <strong>{u.username}</strong>
                  <small className="dx-muted">{u.email}</small>
                </span>
                <Icon name="plus" size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {invite.selected.length > 0 && (
        <div className="dx-chips" aria-label={t('room.convite.seleccionados')}>
          {invite.selected.map((u) => (
            <span key={u.id} className="dx-chip">
              {u.username}
              <IconButton icon="x" bare label={t('room.convite.retirar', { nome: u.username })} onClick={() => invite.unselect(u.id)} />
            </span>
          ))}
        </div>
      )}
      {invite.status && <Alert tone={invite.status.tone}>{invite.status.text}</Alert>}
    </Dialog>
  )
}
