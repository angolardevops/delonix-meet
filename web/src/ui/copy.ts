/**
 * Copiar para a área de transferência — **um mecanismo só**, com *fallback*.
 *
 * PORQUE EXISTE: havia oito `navigator.clipboard.writeText(...)` espalhados,
 * com TRÊS comportamentos diferentes na falha (dois silenciosos, um com
 * mensagem de erro) e dois tempos de reposição do estado «copiado». E nenhum
 * tinha *fallback*.
 *
 * O *fallback* é o que torna isto necessário e não apenas arrumado: o
 * `navigator.clipboard` **não existe fora de um contexto seguro** (a
 * especificação exige HTTPS, `localhost` ou `file:`). O laboratório deste repo
 * corre em **http numa LAN** — logo, nos oito sítios, carregar em «copiar»
 * não fazia **nada** e dois deles engoliam a falha em silêncio. Quem está a
 * copiar o PIN de um ramal ou o token de um gateway, que aparecem UMA vez, não
 * tem como saber que não copiou.
 *
 * O caminho de trás usa o `document.execCommand('copy')` sobre um `textarea`
 * fora do ecrã. É obsoleto na especificação e funciona em http em todos os
 * browsers que este produto suporta — é exactamente para isto que serve.
 */

import { useCallback, useRef, useState } from 'react'

/** O que o mecanismo precisa do ambiente. Injectável, para ser testável. */
export interface AmbienteDeCopia {
  /** `navigator.clipboard`, ou `undefined` fora de contexto seguro. */
  clipboard?: { writeText(texto: string): Promise<void> }
  /** O caminho de trás. `undefined` onde não houver `document`. */
  execCopy?: (texto: string) => boolean
}

/** O ambiente real do browser. */
export function ambienteDoBrowser(): AmbienteDeCopia {
  return {
    clipboard: typeof navigator !== 'undefined' ? navigator.clipboard : undefined,
    execCopy:
      typeof document !== 'undefined'
        ? (texto: string) => {
            // Fora do ecrã mas FOCÁVEL: um `display: none` não se selecciona,
            // e sem selecção o `execCommand` não copia nada.
            const ta = document.createElement('textarea')
            ta.value = texto
            ta.setAttribute('readonly', '')
            ta.style.position = 'fixed'
            ta.style.top = '-1000px'
            ta.style.opacity = '0'
            document.body.appendChild(ta)
            try {
              ta.select()
              ta.setSelectionRange(0, texto.length)
              return document.execCommand('copy')
            } catch {
              return false
            } finally {
              ta.remove()
            }
          }
        : undefined,
  }
}

/**
 * Copia `texto`. Devolve `true` se algum dos dois caminhos funcionou.
 *
 * Tenta primeiro a API moderna; se ela não existir **ou lançar** (permissão
 * negada, documento sem foco), cai no `execCommand`. Lançar não é o mesmo que
 * não existir, e ambos acontecem na prática.
 */
export async function copiarTexto(texto: string, amb: AmbienteDeCopia = ambienteDoBrowser()): Promise<boolean> {
  if (amb.clipboard) {
    try {
      await amb.clipboard.writeText(texto)
      return true
    } catch {
      // Segue para o caminho de trás.
    }
  }
  return amb.execCopy ? amb.execCopy(texto) : false
}

/** Quanto tempo o estado «copiado» fica à vista. */
const MOSTRAR_MS = 2000

/**
 * O estado «copiado» com reposição automática, e o `copiar` que o liga ao
 * mecanismo. Quem chama decide o que mostrar — a peça não impõe aparência,
 * porque os oito sítios são botões, ícones e linhas de lista.
 *
 * `copiar` devolve `false` quando NENHUM caminho funcionou, para quem chama
 * poder dizê-lo. Nenhum dos oito sítios o podia fazer antes.
 */
export function useCopiar(): {
  copiar: (texto: string, chave?: string) => Promise<boolean>
  copiado: string | null
} {
  const [copiado, setCopiado] = useState<string | null>(null)
  // O temporizador vive numa ref: dois cliques seguidos não deixam o primeiro
  // apagar o estado do segundo.
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const copiar = useCallback(async (texto: string, chave = '1') => {
    const ok = await copiarTexto(texto)
    if (!ok) return false
    setCopiado(chave)
    if (timer.current) clearTimeout(timer.current)
    timer.current = setTimeout(() => setCopiado((c) => (c === chave ? null : c)), MOSTRAR_MS)
    return true
  }, [])
  return { copiar, copiado }
}
