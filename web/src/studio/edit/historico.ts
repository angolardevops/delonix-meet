/**
 * Desfazer e refazer — uma pilha de ESTADOS, não de operações inversas.
 *
 * Porque estados: cada edição do `projecto.ts` é pura e devolve um objecto
 * novo que partilha tudo o que não mudou. Guardar o estado anterior custa um
 * ponteiro, e não há uma «operação inversa» por cada edição para manter certa.
 *
 * `chave` junta passos: arrastar um cursor de cor produz dezenas de edições,
 * mas para quem carrega em «Desfazer» é UMA. Edições seguidas com a mesma
 * chave substituem o presente em vez de empilhar.
 */
import { editar, Edicao, Projecto } from './projecto'

export const LIMITE = 100

export interface Historico {
  passado: Projecto[]
  presente: Projecto
  futuro: Projecto[]
  /** Chave do último passo, para juntar arrastos. */
  ultimaChave: string | null
}

export function iniciar(p: Projecto): Historico {
  return { passado: [], presente: p, futuro: [], ultimaChave: null }
}

export function aplicar(h: Historico, e: Edicao, chave: string | null = null, agora = Date.now()): Historico {
  const novo = editar(h.presente, e, agora)
  if (novo === h.presente) return h
  if (chave && chave === h.ultimaChave) return { ...h, presente: novo, futuro: [] }
  const passado = [...h.passado, h.presente]
  if (passado.length > LIMITE) passado.splice(0, passado.length - LIMITE)
  return { passado, presente: novo, futuro: [], ultimaChave: chave }
}

/** Várias edições como UM passo (ex.: «aplicar sugestão» divide e muda o ganho). */
export function aplicarVarias(h: Historico, es: Edicao[], agora = Date.now()): Historico {
  let p = h.presente
  for (const e of es) p = editar(p, e, agora)
  if (p === h.presente) return h
  const passado = [...h.passado, h.presente]
  if (passado.length > LIMITE) passado.splice(0, passado.length - LIMITE)
  return { passado, presente: p, futuro: [], ultimaChave: null }
}

export function podeDesfazer(h: Historico): boolean {
  return h.passado.length > 0
}

export function podeRefazer(h: Historico): boolean {
  return h.futuro.length > 0
}

export function desfazer(h: Historico): Historico {
  if (!h.passado.length) return h
  const anterior = h.passado[h.passado.length - 1]
  return { passado: h.passado.slice(0, -1), presente: anterior, futuro: [h.presente, ...h.futuro], ultimaChave: null }
}

export function refazer(h: Historico): Historico {
  if (!h.futuro.length) return h
  const [seguinte, ...resto] = h.futuro
  return { passado: [...h.passado, h.presente], presente: seguinte, futuro: resto, ultimaChave: null }
}
