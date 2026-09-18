/**
 * Definições pessoais: conta, aparência (tema e língua), segurança (MFA) e
 * marca (nome da aplicação e fundo do ecrã de entrada, guardados neste
 * browser).
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, downloadMyData, updateMe, User } from '../api'
import { getAppName, getLoginBg, setAppName, setLoginBg } from '../branding'
import { currentLang, Lang, LANG_NAMES, LANGS, serverLocale, setLanguage } from '../i18n'
import { applyTheme, storedTheme, Theme } from '../theme'
import { Alert, Button, Dialog, Field, Segmented, Tabs, TextInput } from '../ui/kit'
import MfaPanel from './MfaPanel'
import SessionsPanel from './SessionsPanel'

export type SettingsTab = 'account' | 'appearance' | 'security' | 'brand'

export default function SettingsDialog({
  user,
  initialTab,
  onClose,
  onThemeChange,
}: {
  user: User
  initialTab: SettingsTab
  onClose: () => void
  onThemeChange: (t: Theme) => void
}) {
  const { t } = useTranslation()
  const [tab, setTab] = useState<SettingsTab>(initialTab)

  return (
    <Dialog title={t('shell.definicoes')} onClose={onClose} wide>
      <div className="settings-drawer">
        <Tabs
          label={t('shell.definicoes')}
          value={tab}
          onChange={setTab}
          tabs={[
            { value: 'account', label: t('shell.def.conta') },
            { value: 'appearance', label: t('shell.def.aparencia') },
            { value: 'security', label: t('shell.def.seguranca') },
            { value: 'brand', label: t('shell.def.marca') },
          ]}
        />
        <div className="settings-body">
          {tab === 'account' && <Conta user={user} />}
          {tab === 'appearance' && (
            <div className="settings-grid">
              <Field label={t('shell.def.tema')}>
                <Segmented<Theme>
                  label={t('shell.def.tema')}
                  value={storedTheme()}
                  onChange={(v) => {
                    applyTheme(v)
                    onThemeChange(v)
                  }}
                  options={[
                    { value: 'light', label: t('shell.def.claro') },
                    { value: 'dark', label: t('shell.def.escuro') },
                  ]}
                />
              </Field>
              <Field label={t('shell.def.lingua')}>
                <Segmented<Lang>
                  label={t('shell.def.lingua')}
                  value={currentLang()}
                  onChange={(v) => {
                    void setLanguage(v)
                    // O servidor só guarda pt/en/fr; o chinês fica só neste browser.
                    const server = serverLocale(v)
                    if (server) void updateMe({ locale: server }).catch(() => {})
                  }}
                  options={LANGS.map((l) => ({ value: l, label: <span lang={l}>{LANG_NAMES[l]}</span> }))}
                />
              </Field>
              <p className="dx-muted" style={{ margin: 0 }}>{t('shell.def.salaSempreEscura')}</p>
            </div>
          )}
          {tab === 'security' && (
            <div className="settings-grid">
              <MfaPanel />
              <div>
                <h3 className="dx-eyebrow" style={{ margin: '8px 0' }}>{t('shell.def.sessoesActivas')}</h3>
                <SessionsPanel />
              </div>
            </div>
          )}
          {tab === 'brand' && <Marca />}
        </div>
      </div>
    </Dialog>
  )
}

function Conta({ user }: { user: User }) {
  const { t } = useTranslation()
  const [username, setUsername] = useState(user.username)
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [msg, setMsg] = useState<{ tone: 'success' | 'danger'; text: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [aExportar, setAExportar] = useState(false)
  const [erroExportar, setErroExportar] = useState<string | null>(null)

  async function exportar() {
    setErroExportar(null)
    setAExportar(true)
    try {
      await downloadMyData()
    } catch (e) {
      setErroExportar(apiErrorMessage(e, t('shell.def.exportarErro')))
    } finally {
      setAExportar(false)
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    setMsg(null)
    if (password && password !== confirm) {
      setMsg({ tone: 'danger', text: t('shell.def.passwordsDiferentes') })
      return
    }
    setBusy(true)
    try {
      await updateMe({ username: username.trim() || undefined, password: password || undefined })
      setPassword('')
      setConfirm('')
      setMsg({ tone: 'success', text: t('shell.def.guardado') })
    } catch (err) {
      setMsg({ tone: 'danger', text: apiErrorMessage(err, t('ui.erroGenerico')) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className="settings-grid" onSubmit={submit}>
      <Field label={t('shell.def.email')}>
        <TextInput value={user.email} readOnly disabled />
      </Field>
      <Field label={t('shell.def.nome')} htmlFor="set-name">
        <TextInput id="set-name" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="nickname" />
      </Field>
      <Field label={t('shell.def.novaPassword')} htmlFor="set-pass" hint={t('shell.def.passwordDica')}>
        <TextInput id="set-pass" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" />
      </Field>
      <Field label={t('shell.def.confirmarPassword')} htmlFor="set-pass2">
        <TextInput id="set-pass2" type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} autoComplete="new-password" />
      </Field>
      {msg && <Alert tone={msg.tone}>{msg.text}</Alert>}
      <div>
        <Button type="submit" variant="primary" busy={busy}>
          {t('ui.guardar')}
        </Button>
      </div>
      <div className="settings-section">
        <h3 className="dx-eyebrow" style={{ margin: '8px 0 4px' }}>{t('shell.def.osMeusDados')}</h3>
        <p className="dx-muted" style={{ margin: '0 0 8px' }}>{t('shell.def.exportarDadosDica')}</p>
        {erroExportar && <Alert tone="danger">{erroExportar}</Alert>}
        <Button type="button" variant="secondary" icon="download" busy={aExportar} onClick={exportar}>
          {aExportar ? t('shell.def.aExportar') : t('shell.def.exportarDados')}
        </Button>
      </div>
    </form>
  )
}

function Marca() {
  const { t } = useTranslation()
  const [name, setName] = useState(getAppName())
  const [bg, setBg] = useState(getLoginBg())
  const [erro, setErro] = useState<string | null>(null)
  return (
    <div className="settings-grid">
      <Field label={t('shell.def.nomeApp')} htmlFor="brand-name" hint={t('shell.def.nomeAppDica')}>
        <TextInput id="brand-name" value={name} onChange={(e) => setName(e.target.value)} onBlur={() => setAppName(name)} />
      </Field>
      <Field label={t('shell.def.fundoEntrada')} hint={t('shell.def.fundoEntradaDica')}>
        <input
          type="file"
          accept="image/*"
          onChange={(e) => {
            const f = e.target.files?.[0]
            if (!f) return
            if (f.size > 1_500_000) {
              setErro(t('shell.def.imagemGrande'))
              return
            }
            setErro(null)
            const r = new FileReader()
            r.onload = () => {
              setLoginBg(String(r.result))
              setBg(String(r.result))
            }
            r.readAsDataURL(f)
          }}
        />
      </Field>
      {erro && <Alert tone="danger">{erro}</Alert>}
      {bg && (
        <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
          <img src={bg} alt="" style={{ width: 120, height: 68, objectFit: 'cover', borderRadius: 'var(--r-2)' }} />
          <Button variant="secondary" size="sm" icon="trash" onClick={() => { setLoginBg(null); setBg(null) }}>
            {t('shell.def.removerFundo')}
          </Button>
        </div>
      )}
      <div>
        <Button variant="primary" onClick={() => setAppName(name)}>{t('ui.guardar')}</Button>
      </div>
    </div>
  )
}
