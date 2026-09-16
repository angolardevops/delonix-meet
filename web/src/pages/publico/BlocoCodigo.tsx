/** Bloco de código com cópia — a área de transferência pode ser recusada, e aí não se finge que copiou. */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'

export default function BlocoCodigo({ codigo, rotulo }: { codigo: string; rotulo?: string }) {
  const { t } = useTranslation()
  const [copiado, setCopiado] = useState(false)
  useEffect(() => {
    if (!copiado) return
    const id = setTimeout(() => setCopiado(false), 1500)
    return () => clearTimeout(id)
  }, [copiado])

  function copiar() {
    navigator.clipboard
      ?.writeText(codigo)
      .then(() => setCopiado(true))
      .catch(() => setCopiado(false))
  }

  return (
    <div className="pub-codigo">
      {rotulo && <div className="pub-codigo__rotulo dx-eyebrow">{rotulo}</div>}
      <pre tabIndex={0}>
        <code>{codigo}</code>
      </pre>
      <button type="button" className="pub-codigo__copiar" onClick={copiar} aria-label={t('publico.api.copiar')} title={t('publico.api.copiar')}>
        <Icon name={copiado ? 'check' : 'copy'} size={14} />
        <span aria-live="polite" className={copiado ? undefined : 'dx-sr-only'}>
          {copiado ? t('ui.copiado') : ''}
        </span>
      </button>
    </div>
  )
}
