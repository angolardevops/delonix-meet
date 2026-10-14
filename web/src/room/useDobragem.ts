import { useEffect, useRef, useState } from 'react'
import type { RoomCore } from './useRoomCore'

/**
 * A dobragem ao vivo, vista deste dispositivo (ADR-0030).
 *
 * Duas coisas distintas, e a distinção importa:
 *
 * - **que línguas a sala tem ligadas** — decisão do ANFITRIÃO, porque gasta
 *   inferência da organização (uma cadeia ASR→LLM→TTS por língua);
 * - **qual delas este dispositivo ouve** — decisão de quem ouve, que não gasta
 *   nada e por isso não pede permissão a ninguém.
 *
 * A escolha de ouvir guarda-se localmente: voltar a uma sala não deve obrigar
 * a escolher outra vez. A das línguas activas NÃO se guarda — é da sala, e
 * chega do servidor.
 */
export function useDobragem(core: RoomCore) {
  const { signal } = core
  const [activas, setActivas] = useState<string[]>([])
  const [erro, setErro] = useState<string | null>(null)
  const [ouvir, setOuvirEstado] = useState(() => localStorage.getItem('dx_dub_lang') ?? '')
  const ouvirRef = useRef(ouvir)
  ouvirRef.current = ouvir

  useEffect(() => {
    const off = signal.on('dub-languages', (m: { langs: string[]; error?: string }) => {
      setActivas(m.langs)
      setErro(m.error ?? null)
      // A língua que este dispositivo ouvia deixou de estar ligada: volta ao
      // original em vez de ficar calado à espera de pacotes que não vêm.
      if (ouvirRef.current && !m.langs.includes(ouvirRef.current)) {
        setOuvirEstado('')
        localStorage.removeItem('dx_dub_lang')
        signal.send({ type: 'dub-listen', lang: null })
      }
    })
    return () => off()
  }, [signal])

  function setOuvir(lang: string) {
    setOuvirEstado(lang)
    if (lang) localStorage.setItem('dx_dub_lang', lang)
    else localStorage.removeItem('dx_dub_lang')
    signal.send({ type: 'dub-listen', lang: lang || null })
  }

  /** Só o anfitrião: liga ou desliga uma língua NA SALA. */
  function setLingua(lang: string, on: boolean) {
    signal.send({ type: 'dub-language', lang, on })
  }

  return { activas, ouvir, setOuvir, setLingua, erro }
}
