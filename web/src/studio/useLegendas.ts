/**
 * Legendas ao vivo NO PALCO: a voz de quem apresenta, transcrita no próprio
 * dispositivo e queimada na imagem.
 *
 * O motor é o `Transcriber` de `media.ts` com `preferLocal` — o Whisper WASM
 * do `whisperWorker.ts`, 100% local. NÃO se usa a Web Speech do Chrome: essa
 * envia o áudio para servidores da Google, e o Estúdio promete que a aula não
 * sai da máquina (é o mesmo raciocínio do recorte de fundo).
 *
 * O áudio é o do microfone escolhido na mistura, quando ela já está montada (a
 * gravar ou no ar); antes disso o motor abre o microfone por si.
 */
import { MutableRefObject, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Transcriber } from '../media'
import type { CompositorDeAula } from './compositor'

/** Cada frase fica este tempo no ecrã, se não vier outra. */
const DURACAO_MS = 7000
/** Sem frase nenhuma ao fim disto, o modelo provavelmente não está instalado. */
const LIMITE_PREPARACAO_MS = 90_000

export type EstadoDasLegendas = 'desligadas' | 'a-preparar' | 'activas' | 'sem-modelo'

export function useLegendas(compRef: MutableRefObject<CompositorDeAula | null>, ligadas: boolean, fluxoMontado: boolean) {
  const { i18n } = useTranslation()
  const [estado, setEstado] = useState<EstadoDasLegendas>('desligadas')
  const limpar = useRef<number | null>(null)
  const lingua = i18n.language === 'en' ? 'en' : i18n.language === 'fr' ? 'fr' : 'pt'

  useEffect(() => {
    const c = compRef.current
    if (!ligadas || !c) {
      setEstado('desligadas')
      if (c) c.legenda = ''
      return
    }
    setEstado('a-preparar')
    const tr = new Transcriber()
    let recebeu = false
    const vigia = window.setTimeout(() => {
      if (!recebeu) setEstado('sem-modelo')
    }, LIMITE_PREPARACAO_MS)
    tr.onFinal = (texto) => {
      recebeu = true
      setEstado('activas')
      if (!compRef.current) return
      compRef.current.legenda = texto
      if (limpar.current) window.clearTimeout(limpar.current)
      limpar.current = window.setTimeout(() => {
        if (compRef.current) compRef.current.legenda = ''
      }, DURACAO_MS)
    }
    // As parciais do Whisper local são avisos de estado em português fixo
    // («a carregar modelo…»), não fala: não vão para a imagem.
    tr.onInterim = null
    tr.onError = () => setEstado('sem-modelo')
    tr.start(lingua, c.fluxoDoMicrofone, true)
    return () => {
      window.clearTimeout(vigia)
      if (limpar.current) window.clearTimeout(limpar.current)
      tr.stop()
      if (compRef.current) compRef.current.legenda = ''
    }
    // Recomeça quando a mistura monta: passa a ouvir o microfone escolhido.
  }, [compRef, ligadas, lingua, fluxoMontado])

  return estado
}
