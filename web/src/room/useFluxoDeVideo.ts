/**
 * Chegam QUADROS a este `<video>`?
 *
 * Porque existe: o retrato de um participante escondia o `<video>` quando não
 * havia faixa ou quando a câmara vinha anunciada como desligada — e isso cobre
 * o caso simples. O que não cobria é o que apareceu em produção a 2026-10-13: a
 * faixa EXISTE, a pessoa anuncia a câmara ligada, e mesmo assim nunca chega um
 * quadro. O elemento fica preto, e quem está do outro lado não tem como
 * distinguir isso de uma avaria nossa.
 *
 * Aconteceu com um participante em macOS. A permissão ao nível do SISTEMA
 * (Definições → Privacidade e Segurança → Câmara) pode estar negada enquanto a
 * do browser está concedida: o `getUserMedia` devolve uma faixa que nunca
 * produz imagem.
 *
 * Como se mede, e porquê assim: o `videoWidth` só deixa de ser 0 quando o
 * primeiro quadro é descodificado — é o sinal mais directo de que há imagem.
 * Para o caso de a imagem parar DEPOIS de começar, olha-se para o
 * `currentTime`, que num `MediaStream` só avança enquanto houver quadros.
 *
 * A regra vive numa função PURA (`proximoFluxo`) e o hook é o invólucro: é o
 * que permite testá-la sem montar React nem fingir um elemento de media.
 */
import { RefObject, useEffect, useState } from 'react'

/** Tempo sem um único quadro antes de assumir que não vem nenhum. */
export const ESPERA_PRIMEIRO_QUADRO_MS = 4000
/** Tempo com a imagem parada antes de a dar por perdida. */
export const ESPERA_IMAGEM_PARADA_MS = 6000

export type FluxoDeVideo = 'a-chegar' | 'parado' | 'sem-imagem'

export interface Relogio {
  primeiro: number
  parado: number
}

/** O que se guarda entre observações. */
export interface Vigia {
  ultimoTempo: number
  mexeuEm: number
  jaHouveImagem: boolean
}

export const vigiaNovo = (agora: number): Vigia => ({ ultimoTempo: -1, mexeuEm: agora, jaHouveImagem: false })

/**
 * A regra, sem React e sem relógio do sistema.
 *
 * O prazo é mais longo depois de já ter havido imagem (`parado`) do que antes
 * do primeiro quadro (`sem-imagem`): uma chamada demora segundos a assentar, e
 * um rótulo a piscar a cada reconexão seria pior do que o preto.
 */
export function proximoFluxo(
  leitura: { videoWidth: number; currentTime: number },
  vigia: Vigia,
  agora: number,
  relogio: Relogio,
): { fluxo: FluxoDeVideo; vigia: Vigia } {
  if (!leitura.videoWidth) {
    const parado = agora - vigia.mexeuEm >= relogio.primeiro
    return { fluxo: parado ? 'sem-imagem' : 'a-chegar', vigia }
  }
  if (leitura.currentTime !== vigia.ultimoTempo) {
    return {
      fluxo: 'a-chegar',
      vigia: { ultimoTempo: leitura.currentTime, mexeuEm: agora, jaHouveImagem: true },
    }
  }
  const limite = vigia.jaHouveImagem ? relogio.parado : relogio.primeiro
  return { fluxo: agora - vigia.mexeuEm >= limite ? 'parado' : 'a-chegar', vigia }
}

export function useFluxoDeVideo(
  ref: RefObject<HTMLVideoElement | null>,
  activo: boolean,
  relogio: Relogio = { primeiro: ESPERA_PRIMEIRO_QUADRO_MS, parado: ESPERA_IMAGEM_PARADA_MS },
): FluxoDeVideo {
  const [fluxo, setFluxo] = useState<FluxoDeVideo>('a-chegar')

  useEffect(() => {
    // Sem faixa ou com a câmara anunciada desligada não há nada a vigiar — o
    // retrato já sabe o que mostrar nesse caso.
    if (!activo) {
      setFluxo('a-chegar')
      return
    }
    const el = ref.current
    if (!el) return
    setFluxo('a-chegar')
    let vigia = vigiaNovo(Date.now())
    const id = setInterval(() => {
      const r = proximoFluxo({ videoWidth: el.videoWidth, currentTime: el.currentTime }, vigia, Date.now(), relogio)
      vigia = r.vigia
      setFluxo(r.fluxo)
    }, 1000)
    return () => clearInterval(id)
  }, [ref, activo, relogio])

  return fluxo
}
