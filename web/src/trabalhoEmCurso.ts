/**
 * «Há media em curso que navegar mataria» — uma gravação do Estúdio ou uma
 * emissão em directo.
 *
 * Existe porque o estado vive DENTRO do `Studio` e quem precisa de o saber está
 * FORA dele: a paleta de comandos abria em nova aba para não interromper uma
 * CHAMADA (`inRoom`) e não fazia o mesmo por uma EMISSÃO, por isso um Ctrl+K
 * durante o directo navegava por cima dele. Subir o estado pelo `App` até à
 * paleta obrigava a passar uma prop por três componentes que não lhe querem
 * saber; um store de uma linha diz-se onde se sabe e lê-se onde se precisa.
 *
 * Com `useSyncExternalStore`: é uma subscrição a algo de fora do React, e é
 * assim que o React 19 a quer — sem `useState` + `useEffect`, que dá um primeiro
 * paint com o valor errado.
 *
 * NÃO é autorização nem é durável: é um aviso de interface. Quem fecha a janela
 * à força fecha-a.
 */
import { useSyncExternalStore } from 'react'

let emCurso = false
const ouvintes = new Set<() => void>()

/** Quem tem o estado di-lo aqui (o `Studio`, num efeito). */
export function marcarTrabalhoEmCurso(valor: boolean): void {
  if (valor === emCurso) return
  emCurso = valor
  for (const f of ouvintes) f()
}

export function subscreverTrabalhoEmCurso(f: () => void): () => void {
  ouvintes.add(f)
  return () => {
    ouvintes.delete(f)
  }
}

export const haTrabalhoEmCurso = (): boolean => emCurso

/** O valor no servidor (SSR) é sempre `false`: não há media a correr lá. */
export const useTrabalhoEmCurso = (): boolean =>
  useSyncExternalStore(subscreverTrabalhoEmCurso, haTrabalhoEmCurso, () => false)
