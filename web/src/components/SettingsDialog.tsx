/**
 * Definições pessoais: conta, aparência (tema e língua), segurança (MFA) e
 * marca (nome da aplicação e fundo do ecrã de entrada, guardados neste
 * browser).
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, updateMe, User } from '../api'
import { getAppName, getLoginBg, setAppName, setLoginBg } from '../branding'
import { Lang, LANGS, setLanguage } from '../i18n'
import { applyTheme, storedTheme, Theme } from '../theme'
import { Alert, Button, Dialog, Field, Segmented, Tabs, TextInput } from '../ui/kit'
import MfaPanel from './MfaPanel'

export type SettingsTab = 'account' | 'appearance' | 'security' | 'brand'

const LANG_LABEL: Record<Lang, string> = { pt: 'Português', en: 'English', fr: 'Français' }

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
  const { t, i18n } = useTranslation()
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
                  value={(i18n.language as Lang) ?? 'pt'}
                  onChange={(v) => {
                    void setLanguage(v)
                    void updateMe({ locale: v }).catch(() => {})
                  }}
                  options={LANGS.map((l) => ({ value: l, label: LANG_LABEL[l] }))}
                />
              </Field>
              <p className="dx-muted" style={{ margin: 0 }}>{t('shell.def.salaSempreEscura')}</p>
            </div>
          )}
          {tab === 'security' && <MfaPanel />}
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
