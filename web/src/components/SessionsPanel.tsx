/**
 * Sessões activas da conta: um dispositivo ligado por linha, com a
 * possibilidade de terminar qualquer uma à distância. Não tenta adivinhar o
 * aparelho a partir do user-agent — mostra-o tal como o browser o enviou.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AccountSession, apiErrorMessage, listSessions, revokeSession } from '../api'
import { Alert, Button, Spinner, StatusBadge } from '../ui/kit'
import { currentLang } from '../i18n'

function quando(iso: string) {
  return new Date(iso).toLocaleString(currentLang(), { dateStyle: 'medium', timeStyle: 'short' })
}

export default function SessionsPanel() {
  const { t } = useTranslation()
  const [sessions, setSessions] = useState<AccountSession[] | null>(null)
  const [erro, setErro] = useState<string | null>(null)
  const [aRevogar, setARevogar] = useState<string | null>(null)

  const carregar = () =>
    listSessions()
      .then(setSessions)
      .catch((e) => setErro(apiErrorMessage(e, t('ui.erroCarregar'))))

  useEffect(() => {
    void carregar()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function terminar(sessionId: string) {
    setErro(null)
    setARevogar(sessionId)
    try {
      await revokeSession(sessionId)
      await carregar()
    } catch (e) {
      setErro(apiErrorMessage(e, t('ui.erroGenerico')))
    } finally {
      setARevogar(null)
    }
  }

  if (!sessions && !erro) return <Spinner label={t('ui.aCarregar')} />

  return (
    <div className="sessions-panel">
      {erro && <Alert tone="danger">{erro}</Alert>}
      <p className="dx-muted" style={{ margin: 0 }}>{t('shell.def.sessoes.explicacao')}</p>
      {sessions && sessions.length === 0 && <p className="dx-muted">{t('shell.def.sessoes.vazio')}</p>}
      {sessions && sessions.length > 0 && (
        <ul className="sessions-list">
          {sessions.map((s) => (
            <li key={s.session_id} className="sessions-list__item">
              <div className="sessions-list__info">
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span className="sessions-list__agent" title={s.user_agent ?? undefined}>
                    {s.user_agent || t('shell.def.sessoes.agenteDesconhecido')}
                  </span>
                  {s.current && (
                    <StatusBadge tone="success">{t('shell.def.sessoes.actual')}</StatusBadge>
                  )}
                </div>
                <span className="dx-muted dx-num">
                  {s.ip_address || t('shell.def.sessoes.ipDesconhecido')} · {t('shell.def.sessoes.desde', { quando: quando(s.started_at) })} · {t('shell.def.sessoes.ultimoUso', { quando: quando(s.last_used_at) })}
                </span>
              </div>
              <Button
                variant="secondary"
                size="sm"
                busy={aRevogar === s.session_id}
                onClick={() => terminar(s.session_id)}
              >
                {t('shell.def.sessoes.terminar')}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
