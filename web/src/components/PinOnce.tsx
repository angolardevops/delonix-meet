/**
 * O PIN de um ramal, mostrado UMA vez (R276). Serve à consola (ramal da
 * empresa) e à área da pessoa («o meu ramal»): só o valor e um botão de copiar.
 * Quem o usa diz, ao lado, que o PIN não volta a aparecer.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { GeneratedPin } from '../api'
import { Button } from '../ui/kit'
import { copiarTexto } from '../ui/copy'

export default function PinOnce({ generated, onCopied }: { generated: GeneratedPin; onCopied?: () => void }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  async function copy() {
    try {
      // O mecanismo único (`ui/copy`): com *fallback* para http, onde o
      // `navigator.clipboard` não existe e isto não copiava nada.
      if (await copiarTexto(generated.pin)) setCopied(true)
    } catch {
      // Sem clipboard (contexto não seguro, permissão negada): o PIN está à
      // vista e selecciona-se inteiro com um toque.
    }
    onCopied?.()
  }

  return (
    <div className="org-pin-once" data-testid="ramal-pin">
      <span className="org-pin-once__value dx-num">{generated.pin}</span>
      <Button variant="secondary" size="sm" icon={copied ? 'check' : 'copy'} onClick={() => void copy()}>
        {copied ? t('ui.copiado') : t('consola.ramais.pin.copiar')}
      </Button>
    </div>
  )
}
