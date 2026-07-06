import { createRoot } from 'react-dom/client'
import App from './App'
// Fontes do design system (self-hosted — nada sai da rede local).
import '@fontsource-variable/space-grotesk'
import '@fontsource-variable/instrument-sans'
import '@fontsource/ibm-plex-mono/400.css'
import '@fontsource/ibm-plex-mono/500.css'
import './i18n'
import './styles.css'

createRoot(document.getElementById('root')!).render(<App />)
