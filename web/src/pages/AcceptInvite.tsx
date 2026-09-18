/**
 * Aceitar um convite de organização por link (`#/invite/:token`) — sem
 * sessão; o token é a credencial. GET /api/invites/{token} diz se está
 * pendente, expirado, revogado ou já aceite; POST .../accept cria a conta
 * e devolve uma sessão a sério (mesma persistência de `registerOrg`/`login`),
 * por isso quem aceita entra logo na app, sem ter de fazer login a seguir.
 *
 * Sem fusão de contas: se já existir uma conta com este email, o servidor
 * recusa (409) e esta página manda a pessoa fazer login normal — ver
 * `org::accept_invite` no servidor para o porquê.
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { acceptInvite, apiErrorMessage, getInvitePublic, InvitePublic, User } from '../api'
import { Alert, Button, Card, Empty, Field, Spinner, TextInput } from '../ui/kit'
import CampoPalavraPasse from './auth/CampoPalavraPasse'
import Moldura from './publico/Moldura'

type Estado =
  | { k: 'aVerificar' }
  | { k: 'pronta'; info: InvitePublic }
  | { k: 'indisponivel'; motivo: string }

const estadoHttp = (e: unknown) => (e as { status?: number } | null)?.status

export default function AcceptInvite({ token, onAccepted }: { token: string; onAccepted: (u: User) => void }) {
  const { t } = useTranslation()
  const [estado, setEstado] = useState<Estado>({ k: 'aVerificar' })
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  useEffect(() => {
    let cancelado = false
    setEstado({ k: 'aVerificar' })
    getInvitePublic(token)
      .then((info) => {
        if (cancelado) return
        if (info.revoked) setEstado({ k: 'indisponivel', motivo: t('org.convite.revogado') })
        else if (info.accepted) setEstado({ k: 'indisponivel', motivo: t('org.convite.jaAceite') })
        else if (info.expired) setEstado({ k: 'indisponivel', motivo: t('org.convite.expirado') })
        else setEstado({ k: 'pronta', info })
      })
      .catch((e) => {
        if (cancelado) return
        const s = estadoHttp(e)
        setEstado({ k: 'indisponivel', motivo: s === 404 ? t('org.convite.invalido') : t('org.convite.erro') })
      })
    return () => {
      cancelado = true
    }
  }, [token, t])

  const usernameOk = username.trim().length >= 2
  const passwordOk = password.length >= 8 && password.length <= 128
  const valid = usernameOk && passwordOk

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!valid) return
    setBusy(true)
    setErr('')
    try {
      const user = await acceptInvite(token, { username: username.trim(), password })
      onAccepted(user)
    } catch (x) {
      setErr(apiErrorMessage(x, t('org.convite.erroAceitar')))
      setBusy(false)
    }
  }

  return (
    <Moldura pagina="invite" estreita>
      {estado.k === 'aVerificar' && (
        <div className="pub-espera" role="status">
          <Spinner />
          <span>{t('org.convite.aVerificar')}</span>
        </div>
      )}

      {estado.k === 'indisponivel' && (
        <Card className="pub-cartao">
          <Empty
            icon="userPlus"
            title={t('org.convite.indisponivel')}
            action={
              <a href="#/" className="dx-btn dx-btn--secondary">
                {t('org.convite.irEntrada')}
              </a>
            }
          >
            {estado.motivo}
          </Empty>
        </Card>
      )}

      {estado.k === 'pronta' && (
        <Card className="pub-cartao">
          <form className="org-form" onSubmit={submit}>
            <div className="dx-eyebrow">{t('org.convite.convidadoPara', { org: estado.info.org_name })}</div>
            <h1>{t('org.convite.titulo')}</h1>
            <dl className="dx-kv">
              <dt>{t('org.campo.email')}</dt>
              <dd className="dx-num">{estado.info.email}</dd>
              <dt>{t('org.campo.papel')}</dt>
              <dd>{estado.info.role === 'admin' ? t('org.papel.admin') : t('org.papel.membro')}</dd>
            </dl>
            <Field label={t('org.campo.nome')} htmlFor="invite-username">
              <TextInput
                id="invite-username"
                autoFocus
                required
                autoComplete="name"
                maxLength={64}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </Field>
            <Field
              label={t('org.campo.palavraPasse')}
              htmlFor="invite-password"
              hint={t('org.convite.palavraPasseDica')}
              error={password && !passwordOk ? t('org.membro.palavraPasseCurta') : undefined}
            >
              <CampoPalavraPasse
                id="invite-password"
                required
                autoComplete="new-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                aria-invalid={password !== '' && !passwordOk}
              />
            </Field>
            {err && <Alert tone="danger">{err}</Alert>}
            <Button type="submit" variant="primary" size="lg" block busy={busy} disabled={!valid}>
              {t('org.convite.entrar')}
            </Button>
          </form>
        </Card>
      )}
    </Moldura>
  )
}
