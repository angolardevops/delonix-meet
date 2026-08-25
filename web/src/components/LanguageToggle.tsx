import { useTranslation } from 'react-i18next'
import { Lang, setLanguage } from '../i18n'

/**
 * Toggle global PT/EN/FR — persiste em localStorage E na BD (sincroniza entre
 * dispositivos). Saiu do Shell: o Login e a Landing usavam-no e arrastavam a
 * consola inteira para o chunk de arranque (achado 1.2).
 */
export default function LanguageToggle() {
  const { i18n } = useTranslation()
  const lang: Lang = i18n.language.startsWith('en') ? 'en' : i18n.language.startsWith('fr') ? 'fr' : 'pt'

  function pick(l: Lang) {
    void setLanguage(l)
    // Persiste à BD em best-effort — falha silenciosa para não bloquear a UI.
    void import('../api').then(({ updateMe }) => updateMe({ locale: l })).catch(() => {})
  }

  return (
    <div className="lang-toggle" role="group" aria-label="Idioma / Language">
      {(['pt', 'en', 'fr'] as const).map((l) => (
        <button
          key={l}
          className={lang === l ? 'lang-btn active' : 'lang-btn'}
          aria-pressed={lang === l}
          onClick={() => pick(l)}
        >
          {l.toUpperCase()}
        </button>
      ))}
    </div>
  )
}
