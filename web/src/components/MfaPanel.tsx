/**
 * Segundo factor por TOTP. Três momentos com regras próprias:
 *  - inscrever: o segredo e o QR aparecem UMA vez;
 *  - activar: pede um código do autenticador e devolve 10 códigos de
 *    recuperação, também UMA vez — «Concluir» só fica activo depois de a
 *    pessoa confirmar que os guardou;
 *  - desactivar: exige um código válido (uma sessão roubada não chega).
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, mfaActivar, mfaDesactivar, mfaEstado, MfaEstado, mfaInscrever } from '../api'
import { Alert, Button, Checkbox, Field, Spinner, StatusBadge, TextInput } from '../ui/kit'

type Passo =
  | { k: 'estado' }
  | { k: 'inscrever'; secret: string; qr: string | null }
  | { k: 'codigos'; codes: string[] }
  | { k: 'desactivar' }

export default function MfaPanel() {
  const { t } = useTranslation()
  const [estado, setEstado] = useState<MfaEstado | null>(null)
  const [passo, setPasso] = useState<Passo>({ k: 'estado' })
  const [code, setCode] = useState('')
  const [erro, setErro] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [guardei, setGuardei] = useState(false)

  const carregar = () =>
    mfaEstado()
      .then(setEstado)
      .catch((e) => setErro(apiErrorMessage(e, t('ui.erroCarregar'))))

  useEffect(() => {
    void carregar()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function inscrever() {
    setErro(null)
    setBusy(true)
    try {
      const r = await mfaInscrever()
      let qr: string | null = null
      try {
        const { toString } = await import('qrcode')
        qr = await toString(r.otpauth_uri, { type: 'svg', margin: 1, width: 200 })
      } catch {
        /* sem QR, o segredo em texto continua a servir */
      }
      setPasso({ k: 'inscrever', secret: r.secret, qr })
      setCode('')
    } catch (e) {
      setErro(apiErrorMessage(e, t('ui.erroGenerico')))
    } finally {
      setBusy(false)
    }
  }

  async function activar() {
    setErro(null)
    setBusy(true)
    try {
      const r = await mfaActivar(code.trim())
      setPasso({ k: 'codigos', codes: r.backup_codes })
      setGuardei(false)
    } catch (e) {
      setErro(apiErrorMessage(e, t('auth.mfa.codigoInvalido')))
    } finally {
      setBusy(false)
    }
  }

  async function desactivar() {
    setErro(null)
    setBusy(true)
    try {
      await mfaDesactivar(code.trim())
      setPasso({ k: 'estado' })
      setCode('')
      await carregar()
    } catch (e) {
      setErro(apiErrorMessage(e, t('auth.mfa.codigoInvalido')))
    } finally {
      setBusy(false)
    }
  }

  if (!estado && !erro) return <Spinner label={t('ui.aCarregar')} />

  return (
    <div className="mfa-panel">
      {erro && <Alert tone="danger">{erro}</Alert>}

      {passo.k === 'estado' && estado && (
        <div className="mfa-estado">
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <strong style={{ flex: 1 }}>{t('auth.mfa.titulo')}</strong>
            {estado.enabled ? (
              <span className="mfa-on">
                <StatusBadge tone="success" icon="check">{t('auth.mfa.activo')}</StatusBadge>
              </span>
            ) : (
              <StatusBadge tone="neutral">{t('auth.mfa.inactivo')}</StatusBadge>
            )}
          </div>
          <p className="dx-muted" style={{ margin: 0 }}>{t('auth.mfa.explicacao')}</p>
          {estado.enabled ? (
            <>
              <p className="dx-muted" style={{ margin: 0 }}>
                {t('auth.mfa.codigosRestantes', { count: estado.backup_codes_left })}
              </p>
              <div>
                <Button variant="secondary" onClick={() => { setPasso({ k: 'desactivar' }); setCode('') }}>
                  {t('auth.mfa.desactivar')}
                </Button>
              </div>
            </>
          ) : (
            <div>
              <Button variant="primary" icon="shieldCheck" busy={busy} onClick={inscrever}>
                {t('auth.mfa.activar')}
              </Button>
            </div>
          )}
        </div>
      )}

      {passo.k === 'inscrever' && (
        <div className="mfa-inscrever">
          <p style={{ margin: 0 }}>{t('auth.mfa.passo1')}</p>
          <div className="mfa-qr-row">
            {passo.qr && <div className="mfa-qr" dangerouslySetInnerHTML={{ __html: passo.qr }} />}
            <div style={{ minWidth: 0 }}>
              <div className="dx-eyebrow">{t('auth.mfa.segredo')}</div>
              <code className="mfa-secret dx-num">{passo.secret}</code>
            </div>
          </div>
          <Field label={t('auth.mfa.passo2')} htmlFor="mfa-code">
            <TextInput
              id="mfa-code"
              inputMode="numeric"
              autoComplete="one-time-code"
              code
              maxLength={6}
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
            />
          </Field>
          <div style={{ display: 'flex', gap: 8 }}>
            <Button variant="secondary" onClick={() => setPasso({ k: 'estado' })}>{t('ui.cancelar')}</Button>
            <Button variant="primary" busy={busy} disabled={code.length !== 6} onClick={activar}>
              {t('auth.mfa.confirmar')}
            </Button>
          </div>
        </div>
      )}

      {passo.k === 'codigos' && (
        <div className="mfa-codigos">
          <Alert tone="warning">{t('auth.mfa.guardaOsCodigos')}</Alert>
          <ul className="mfa-codes dx-num">
            {passo.codes.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
          <div className="mfa-confirm">
            <Checkbox label={t('auth.mfa.jaGuardei')} checked={guardei} onChange={(e) => setGuardei(e.target.checked)} />
          </div>
          <div>
            <Button
              variant="primary"
              disabled={!guardei}
              onClick={() => {
                setPasso({ k: 'estado' })
                void carregar()
              }}
            >
              {t('auth.mfa.concluir')}
            </Button>
          </div>
        </div>
      )}

      {passo.k === 'desactivar' && (
        <div className="mfa-desactivar">
          <Field label={t('auth.mfa.codigoParaDesactivar')} htmlFor="mfa-off">
            <TextInput
              id="mfa-off"
              inputMode="numeric"
              autoComplete="one-time-code"
              code
              value={code}
              onChange={(e) => setCode(e.target.value.trim())}
            />
          </Field>
          <div style={{ display: 'flex', gap: 8 }}>
            <Button variant="secondary" onClick={() => setPasso({ k: 'estado' })}>{t('ui.cancelar')}</Button>
            <Button variant="danger" busy={busy} disabled={code.length < 6} onClick={desactivar}>
              {t('auth.mfa.desactivar')}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
