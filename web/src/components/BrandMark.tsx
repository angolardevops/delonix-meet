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

/**
 * O SÍMBOLO mais o NOME — o conjunto que aparece nos cabeçalhos (R101).
 *
 * O `BrandMark` acima resolveu metade do problema da marca-branca: o símbolo
 * passou a seguir o nome. Mas o NOME continuava escrito à mão ao lado dele —
 * `<BrandMark /> Delonix <span>Meet</span>` — em cinco páginas. Renomear a
 * aplicação trocava o símbolo e deixava o nome antigo colado a ele, que é um
 * resultado pior do que não ter mudado nada.
 *
 * Este componente desenha os dois a partir da mesma fonte.
 */
export function BrandLockup({ big = false, suffix }: { big?: boolean; suffix?: string }) {
  const [partes, setPartes] = useState(appNameParts())
  useEffect(() => {
    const on = () => setPartes(appNameParts())
    window.addEventListener('dx-branding', on)
    return () => window.removeEventListener('dx-branding', on)
  }, [])
  return (
    <>
      <BrandMark big={big} /> {partes[0]} <span>{partes[1]}</span>
      {suffix ? ` ${suffix}` : null}
    </>
  )
}
