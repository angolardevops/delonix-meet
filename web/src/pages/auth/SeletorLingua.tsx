/**
 * Escolha de língua na própria entrada: quem ainda não tem conta não chega às
 * definições. Os nomes escrevem-se NA língua — quem não lê português tem de
 * reconhecer o «English» sem o traduzir.
 */
import { useTranslation } from 'react-i18next'
import { Lang, LANGS, setLanguage } from '../../i18n'
import { Icon } from '../../ui/icons'
import { Select } from '../../ui/kit'

const NOME: Record<Lang, string> = { pt: 'Português', en: 'English', fr: 'Français' }

export default function SeletorLingua() {
  const { t, i18n } = useTranslation()
  const actual = (LANGS as readonly string[]).includes(i18n.language) ? (i18n.language as Lang) : 'pt'
  return (
    <label className="auth-lingua">
      <Icon name="globe" size={14} />
      <span className="dx-sr-only">{t('auth.lingua')}</span>
      <Select value={actual} onChange={(e) => void setLanguage(e.target.value as Lang)} aria-label={t('auth.lingua')}>
        {LANGS.map((l) => (
          <option key={l} value={l} lang={l}>
            {NOME[l]}
          </option>
        ))}
      </Select>
    </label>
  )
}
