import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { applyTheme, storedTheme, Theme } from '../theme'

/** Seletor de tema. Saiu do Shell para o Room o poder usar sem arrastar a consola. */
export default function ThemePicker() {
  const { t } = useTranslation()
  const [theme, setTheme] = useState<Theme>(storedTheme)

  function pick(next: Theme) {
    setTheme(next)
    applyTheme(next)
  }

  return (
    <div className="theme-picker">
      <label className="theme-label">{t('common.theme', 'Tema')}</label>
      <select value={theme} onChange={(e) => pick(e.target.value as Theme)}>
        <option value="default">Delonix · Escuro (Padrão)</option>
        <option value="delonix-light">Delonix · Claro</option>
      </select>
    </div>
  )
}
