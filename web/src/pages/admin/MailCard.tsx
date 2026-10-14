/**
 * O SMTP próprio da organização (ADR-0025, `server/src/org_mail.rs`), e a
 * caixa de saída dela.
 *
 * **Sem linha, ou com ela desligada, o correio sai pelo relay do operador** —
 * é por isso que «não configurado» é um ESTADO NORMAL e não um erro: a rota
 * responde `404` e este cartão mostra o estado, não um alarme.
 *
 * Três coisas que o ecrã tem de respeitar, porque o servidor as impõe:
 *
 * 1. **a password nunca volta** — a leitura só diz `password_configured`, e o
 *    campo fica vazio numa alteração; deixá-lo vazio MANTÉM a que está;
 * 2. **desligar o STARTTLS exige confirmação por escrito** (`allow_plaintext`),
 *    porque sem TLS a password do cliente viaja à vista;
 * 3. **o host passa pela guarda de saída** e um destino interno é recusado com
 *    `org_mail_settings.host_refused` — a mensagem vem do servidor.
 *
 * A VERIFICAÇÃO liga-se ao relay e fala SMTP sem mandar mensagem nenhuma. É o
 * que separa «configurei» de «funciona», e existe porque sem ela o primeiro
 * sinal de erro era um email que nunca chegou.
 */
import { FormEvent, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import {
  ApiError,
  checkOrgMailRelay,
  deleteOrgMailSettings,
  getOrgMailSettings,
  OrgMailSettings,
  putOrgMailSettings,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Confirm, Field, StatusBadge, TextInput, Toggle } from '../../ui/kit'
import { formatDateTime, orgErrorMessage, refusalAware, useLocaleTag } from './orgShared'

/**
 * A razão de uma recusa, traduzida pelo CÓDIGO.
 *
 * O servidor manda a razão em português — todas as mensagens dele são — e numa
 * interface em inglês isso aparecia como «o URL aponta para um endereço
 * interno/privado». Os códigos destas três recusas são estáveis (foram criados
 * para isto, no #325), por isso traduzem-se; qualquer outra coisa cai na
 * mensagem do servidor, que é melhor do que uma genérica.
 */
function razaoTraduzida(e: unknown, t: TFunction): string | null {
  if (!(e instanceof ApiError)) return null
  const code = (e.body as { code?: string } | null)?.code
  switch (code) {
    case 'org_mail_settings.host_refused':
      return t('org.correio.recusaHost')
    case 'org_mail_settings.invalid_from':
      return t('org.correio.recusaRemetente')
    case 'org_mail_settings.plaintext_not_confirmed':
      return t('org.correio.recusaSemTls')
    default:
      return null
  }
}

/** Um rascunho do formulário. A password é só de ESCRITA: vazia = manter. */
interface Rascunho {
  host: string
  port: string
  username: string
  password: string
  from_address: string
  starttls: boolean
  allow_plaintext: boolean
  enabled: boolean
}

const VAZIO: Rascunho = {
  host: '',
  port: '587',
  username: '',
  password: '',
  from_address: '',
  starttls: true,
  allow_plaintext: false,
  enabled: true,
}

function deSettings(s: OrgMailSettings): Rascunho {
  return {
    host: s.host,
    port: String(s.port),
    username: s.username,
    password: '',
    from_address: s.from_address,
    starttls: s.starttls,
    allow_plaintext: !s.starttls,
    enabled: s.enabled,
  }
}

export default function MailCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  // `404` = esta organização não configurou SMTP próprio. É um estado, não um
  // erro: o correio dela sai pelo relay do operador.
  const settings = useAsync(
    (signal) =>
      refusalAware(
        getOrgMailSettings(orgId, signal).catch((e) => {
          if (e instanceof ApiError && e.status === 404) return null
          throw e
        }),
        t,
      ),
    [orgId],
  )
  const [editar, setEditar] = useState(false)
  const [d, setD] = useState<Rascunho>(VAZIO)
  const [guardando, setGuardando] = useState(false)
  const [aApagar, setAApagar] = useState(false)
  const [verificando, setVerificando] = useState(false)
  const [resultado, setResultado] = useState<{ ok: boolean; error: string | null } | null>(null)
  const [err, setErr] = useState('')

  const set = <K extends keyof Rascunho>(k: K, v: Rascunho[K]) => setD((x) => ({ ...x, [k]: v }))

  function abrir(actual: OrgMailSettings | null) {
    setErr('')
    setResultado(null)
    setD(actual ? deSettings(actual) : VAZIO)
    setEditar(true)
  }

  const portaNum = Number(d.port)
  const portaMa = !Number.isInteger(portaNum) || portaNum < 1 || portaNum > 65535
  // O servidor recusa `starttls: false` sem a confirmação escrita; o ecrã
  // não deixa sequer chegar lá.
  const faltaConfirmar = !d.starttls && !d.allow_plaintext
  const podeGuardar = d.host.trim() !== '' && d.from_address.trim() !== '' && !portaMa && !faltaConfirmar

  async function guardar(e: FormEvent) {
    e.preventDefault()
    if (!podeGuardar) return
    setErr('')
    setGuardando(true)
    try {
      await putOrgMailSettings(orgId, {
        host: d.host.trim(),
        port: portaNum,
        username: d.username.trim(),
        // Vazia = MANTER a que está: por isso só se envia quando foi escrita.
        ...(d.password === '' ? {} : { password: d.password }),
        from_address: d.from_address.trim(),
        starttls: d.starttls,
        allow_plaintext: d.allow_plaintext,
        enabled: d.enabled,
      })
      setEditar(false)
      settings.reload()
    } catch (e2) {
      // A razão vem do servidor — `host_refused`, `invalid_from`,
      // `plaintext_not_confirmed` — e não se reescreve aqui.
      setErr(razaoTraduzida(e2, t) ?? orgErrorMessage(e2, t, 'org.correio.erroGuardar'))
    } finally {
      setGuardando(false)
    }
  }

  async function apagar() {
    setErr('')
    try {
      await deleteOrgMailSettings(orgId)
      setAApagar(false)
      setEditar(false)
      setResultado(null)
      settings.reload()
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'org.correio.erroApagar'))
    }
  }

  async function verificar() {
    setErr('')
    setResultado(null)
    setVerificando(true)
    try {
      setResultado(await checkOrgMailRelay(orgId))
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'org.correio.erroVerificar'))
    } finally {
      setVerificando(false)
    }
  }

  return (
    <Card title={t('org.correio.titulo')} eyebrow={t('org.correio.eyebrow')} as="section">
      <div className="org-card-pad" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {err && <Alert tone="danger">{err}</Alert>}
        <AsyncSection state={settings.state} onRetry={settings.reload}>
          {(actual) => (
            <>
              {!editar && (
                <>
                  {actual === null ? (
                    // Não é um alarme: é como a maioria das organizações vive.
                    <p className="dx-muted" style={{ margin: 0 }}>{t('org.correio.semConfiguracao')}</p>
                  ) : (
                    <>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                        <strong className="dx-num">
                          {actual.host}:{actual.port}
                        </strong>
                        <StatusBadge tone={actual.enabled ? 'success' : 'neutral'}>
                          {actual.enabled ? t('org.correio.ligado') : t('org.correio.desligado')}
                        </StatusBadge>
                        <StatusBadge tone={actual.starttls ? 'success' : 'warning'}>
                          {actual.starttls ? 'STARTTLS' : t('org.correio.semTls')}
                        </StatusBadge>
                      </div>
                      <p className="dx-muted" style={{ margin: 0 }}>
                        {t('org.correio.remetente', { de: actual.from_address })}
                        {actual.username ? ` · ${actual.username}` : ''}
                        {actual.password_configured ? ` · ${t('org.correio.comPassword')}` : ''}
                      </p>
                      <p className="dx-muted" style={{ margin: 0, fontSize: 11 }}>
                        {t('org.correio.actualizado', { quando: formatDateTime(actual.updated_at, locale) })}
                      </p>
                      {resultado && (
                        <Alert tone={resultado.ok ? 'success' : 'danger'}>
                          {resultado.ok ? t('org.correio.verificaOk') : resultado.error || t('org.correio.verificaFalhou')}
                        </Alert>
                      )}
                    </>
                  )}
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                    <Button variant="primary" size="sm" icon={actual ? 'edit' : 'plus'} onClick={() => abrir(actual)}>
                      {actual ? t('org.correio.editar') : t('org.correio.configurar')}
                    </Button>
                    {actual && (
                      <>
                        <Button variant="secondary" size="sm" icon="check" busy={verificando} onClick={() => void verificar()}>
                          {t('org.correio.verificar')}
                        </Button>
                        <Button variant="ghost" size="sm" icon="trash" onClick={() => setAApagar(true)}>
                          {t('org.correio.apagar')}
                        </Button>
                      </>
                    )}
                  </div>
                  {!actual && (
                    <p className="dx-muted" style={{ margin: 0, fontSize: 11 }}>{t('org.correio.dicaFallback')}</p>
                  )}
                </>
              )}

              {editar && (
                <form onSubmit={(e) => void guardar(e)} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                  <Field label={t('org.correio.host')} hint={t('org.correio.hostDica')}>
                    <TextInput value={d.host} onChange={(e) => set('host', e.target.value)} placeholder="smtp.exemplo.ao" required />
                  </Field>
                  <Field label={t('org.correio.porta')}>
                    <TextInput
                      type="number"
                      min={1}
                      max={65535}
                      className="dx-num"
                      value={d.port}
                      onChange={(e) => set('port', e.target.value)}
                      required
                    />
                  </Field>
                  <Field label={t('org.correio.remetenteCampo')} hint={t('org.correio.remetenteDica')}>
                    <TextInput
                      value={d.from_address}
                      onChange={(e) => set('from_address', e.target.value)}
                      placeholder={t('org.correio.remetenteExemplo')}
                      required
                    />
                  </Field>
                  <Field label={t('org.correio.utilizador')}>
                    <TextInput value={d.username} onChange={(e) => set('username', e.target.value)} autoComplete="off" />
                  </Field>
                  <Field
                    label={t('org.correio.password')}
                    hint={actual?.password_configured ? t('org.correio.passwordManter') : t('org.correio.passwordDica')}
                  >
                    <TextInput
                      type="password"
                      value={d.password}
                      onChange={(e) => set('password', e.target.value)}
                      autoComplete="new-password"
                    />
                  </Field>
                  <Toggle label="STARTTLS" checked={d.starttls} onChange={() => set('starttls', !d.starttls)} />
                  {!d.starttls && (
                    <>
                      <Alert tone="warning">{t('org.correio.avisoSemTls')}</Alert>
                      <Toggle
                        label={t('org.correio.confirmarSemTls')}
                        checked={d.allow_plaintext}
                        onChange={() => set('allow_plaintext', !d.allow_plaintext)}
                      />
                    </>
                  )}
                  <Toggle
                    label={t('org.correio.activo')}
                    checked={d.enabled}
                    onChange={() => set('enabled', !d.enabled)}
                  />
                  <p className="dx-muted" style={{ margin: 0, fontSize: 11 }}>{t('org.correio.dicaDesligado')}</p>
                  <div style={{ display: 'flex', gap: 8 }}>
                    <Button type="submit" variant="primary" size="sm" busy={guardando} disabled={!podeGuardar}>
                      {t('ui.guardar')}
                    </Button>
                    <Button type="button" variant="ghost" size="sm" onClick={() => setEditar(false)}>
                      {t('ui.cancelar')}
                    </Button>
                  </div>
                </form>
              )}

              {aApagar && (
                <Confirm
                  title={t('org.correio.apagarTitulo')}
                  confirmLabel={t('org.correio.apagar')}
                  tone="danger"
                  onConfirm={apagar}
                  onClose={() => setAApagar(false)}
                >
                  {t('org.correio.apagarAviso')}
                </Confirm>
              )}
            </>
          )}
        </AsyncSection>
      </div>
    </Card>
  )
}
