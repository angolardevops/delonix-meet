/**
 * A marca. Uma só, e que respeita quem renomeia a aplicação (R100, R101):
 * com o nome de origem desenha-se o símbolo Delonix (o de `/logo.svg`, aqui
 * inline); com outro nome, um quadrado com a inicial — mostrar a flor de
 * Delonix numa instância que se chama «Acme Reuniões» é mostrar a marca errada.
 */
import { useEffect, useState } from 'react'
import { appNameParts, getAppName, isMarcaDeOrigem } from '../branding'
import { DelonixSymbol } from '../ui/icons'

/** Referência ao ficheiro público, para quem precisar do URL (manifest, notificações). */
export const LOGO_URL = '/logo.svg'

function useAppName() {
  const [name, setName] = useState(getAppName())
  useEffect(() => {
    const on = () => setName(getAppName())
    window.addEventListener('dx-branding', on)
    return () => window.removeEventListener('dx-branding', on)
  }, [])
  return name
}

export function BrandMark({ size = 26, tone = 'accent' }: { size?: number; tone?: 'accent' | 'onDark' | 'tile' }) {
  const name = useAppName()
  if (!isMarcaDeOrigem()) {
    return (
      <span className="brand-square" style={{ width: size, height: size, fontSize: Math.round(size * 0.5) }} aria-hidden="true">
        {name.trim().charAt(0).toUpperCase()}
      </span>
    )
  }
  if (tone === 'tile') {
    return (
      <span className="brand-tile" style={{ width: size, height: size }} aria-hidden="true">
        <DelonixSymbol size={Math.round(size * 0.74)} />
      </span>
    )
  }
  return (
    <span className={tone === 'onDark' ? 'brand-symbol brand-symbol--dark' : 'brand-symbol'} aria-hidden="true">
      <DelonixSymbol size={size} />
    </span>
  )
}

/** Símbolo + nome. A segunda palavra leva o acento de marca («Delonix**Meet**»). */
export function BrandLockup({ size = 26, tone = 'accent', display }: { size?: number; tone?: 'accent' | 'onDark' | 'tile'; display?: boolean }) {
  useAppName()
  const [first, rest] = appNameParts()
  return (
    <span className={display ? 'brand-lockup brand-lockup--display' : 'brand-lockup'}>
      <BrandMark size={size} tone={tone} />
      <span className="brand-name">
        {first}
        {rest && <span className="brand-name__accent">{rest}</span>}
      </span>
    </span>
  )
}
