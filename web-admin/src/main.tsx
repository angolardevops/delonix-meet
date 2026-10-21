import { createRoot } from 'react-dom/client'
import './ui/tokens.css'
import './ui/base.css'
import './ui/shell.css'
import App from './App'
import { initLanguage } from './i18n'

// O idioma resolve-se ANTES do primeiro render (sem flash de português).
void initLanguage().finally(() => {
  createRoot(document.getElementById('root')!).render(<App />)
})
