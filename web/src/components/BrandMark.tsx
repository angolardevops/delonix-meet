import { useEffect, useState } from 'react'
import { appNameParts, isMarcaDeOrigem } from '../branding'

/**
 * A marca, num sítio só (R100).
 *
 * Havia duas: o globo de `/logo.svg` na landing, no lobby, no estado, no legal
 * e nos docs; e um quadrado com a inicial no rail da consola. Com o nome de
 * origem isso é apenas incoerente — a mesma aplicação com dois símbolos.
 *
 * O defeito a sério aparece ao RENOMEAR. O quadrado adapta-se (usa a inicial do
 * nome configurado); os cinco ecrãs com `/logo.svg` continuavam a mostrar o
 * globo Delonix. Ou seja: a marca-branca estava feita a meio, e quem a usasse
 * via o logótipo de outra empresa em metade do produto.
 *
 * Este componente decide: logótipo enquanto o nome for o de origem, quadrado
 * com a inicial a partir do momento em que deixar de ser.
 */
export function BrandMark({ big = false }: { big?: boolean }) {
  const [origem, setOrigem] = useState(isMarcaDeOrigem())
  const [partes, setPartes] = useState(appNameParts())
  useEffect(() => {
    const on = () => {
      setOrigem(isMarcaDeOrigem())
      setPartes(appNameParts())
    }
    window.addEventListener('dx-branding', on)
    return () => window.removeEventListener('dx-branding', on)
  }, [])

  if (origem) {
    return <img src="/logo.svg" alt="" className={big ? 'brand-logo big' : 'brand-logo'} />
  }
  return (
    <span className={big ? 'brand-square big' : 'brand-square'} aria-hidden="true">
      {(partes[0] || 'D').trim().charAt(0).toUpperCase()}
    </span>
  )
}
