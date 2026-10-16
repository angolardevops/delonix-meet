/**
 * Os EFEITOS SONOROS do estúdio: as teclas que disparam um genérico, um
 * aplauso, uma vinheta ou uma buzina — e o ducking que baixa a música debaixo
 * da voz.
 *
 * PORQUE É UM MÓDULO PURO: aqui só vive o MODELO (quantas teclas, qual tecla,
 * quando baixar e quanto) e o arquivo dos ficheiros. Tocar é da mesa de som,
 * que é dona do `AudioContext` — um efeito tem de passar pelo fader, pelo
 * medidor e pelo limitador como qualquer outra fonte, senão sai por cima do
 * programa sem ninguém o ver no ecrã.
 *
 * QUATRO TECLAS, e não oito: as sobreposições já ocupam ⌥1–⌥4 e os efeitos
 * ficam em ⌥5–⌥8, na mesma fila e com o mesmo modificador. Oito efeitos
 * obrigariam a inventar uma segunda fila de teclas que não cabe em lado nenhum
 * do teclado sem colidir com o browser (F11, F12) ou com a mesa de corte
 * (1–6, ⇧1–⇧6, F1–F6).
 */

export const MAX_EFEITOS = 4
/** O primeiro número da fila: ⌥5 é o efeito 1. */
export const PRIMEIRA_TECLA = 5

/** O número da tecla (⌥n) de um efeito, pelo índice 0–3. */
export function teclaDoEfeito(indice: number): number {
  return PRIMEIRA_TECLA + indice
}

/** O índice do efeito de uma tecla ⌥n, ou `null` se aquela tecla não é de efeito. */
export function efeitoDaTecla(n: number): number | null {
  const i = n - PRIMEIRA_TECLA
  return i >= 0 && i < MAX_EFEITOS ? i : null
}

export interface Efeito {
  /** `efeito:0` … `efeito:3` — é também o id do canal na mesa. */
  id: string
  nome: string
  /** Em ciclo: uma cama de música ou um ambiente, que fica até se parar. */
  emCiclo: boolean
}

export const idDoEfeito = (indice: number) => `efeito:${indice}`

export function indiceDoEfeito(id: string): number | null {
  const m = /^efeito:([0-3])$/.exec(id)
  return m ? Number(m[1]) : null
}

// ---------------------------------------------------------------- ducking

/**
 * Quanto se baixa a música e os efeitos enquanto alguém fala, em dB.
 *
 * −12 dB é o degrau clássico de rádio: a cama continua a ouvir-se por baixo da
 * voz (não desaparece, que daria um buraco a cada pausa) e deixa de competir
 * com ela.
 */
export const DUCK_DB = -12

/**
 * O limiar, em dBFS RMS, a partir do qual um microfone conta como «a falar».
 *
 * −40 dBFS deixa passar o ruído de sala e a respiração; a porta de cada canal,
 * quando ligada, já limpa o resto. Baixar isto faz a música abanar sozinha, que
 * é pior do que não haver ducking nenhum.
 */
export const LIMIAR_DE_VOZ_DB = -40

/**
 * Há voz? Decide-se pelo nível PRÉ-fader dos microfones: um microfone com o
 * fader em baixo continua a ser alguém a falar, e um microfone MUDO não é.
 *
 * Pura de propósito: é a regra que faz a música mexer sozinha numa emissão, e
 * uma regra dessas não se lê dentro de um `setInterval`.
 */
export function haVoz(niveisDb: readonly number[], limiarDb = LIMIAR_DE_VOZ_DB): boolean {
  return niveisDb.some((db) => Number.isFinite(db) && db > limiarDb)
}

// ---------------------------------------------------------------- arquivo

/**
 * Os ficheiros dos efeitos, em IndexedDB.
 *
 * Base PRÓPRIA, e não a do editor: as teclas do estúdio não podem arrastar o
 * chunk do editor para quem só quer fazer um programa (é o mesmo motivo por
 * que o `EstudioTv` entra por `lazy`). E não `localStorage`, que guarda texto e
 * anda pelos 5 MB — um genérico são megabytes de binário, e o IndexedDB guarda
 * `Blob` nativamente, sem base64 a inflar tudo em mais um terço.
 */
const BD = 'delonix-estudio-efeitos'
const LOJA = 'efeitos'

export interface EfeitoGuardado {
  id: string
  nome: string
  emCiclo: boolean
  ficheiro: Blob
}

function abrir(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const p = indexedDB.open(BD, 1)
    p.onupgradeneeded = () => {
      if (!p.result.objectStoreNames.contains(LOJA)) p.result.createObjectStore(LOJA, { keyPath: 'id' })
    }
    p.onsuccess = () => resolve(p.result)
    p.onerror = () => reject(p.error ?? new Error('IndexedDB indisponível'))
  })
}

async function transaccao<T>(modo: IDBTransactionMode, f: (loja: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const bd = await abrir()
  try {
    return await new Promise<T>((resolve, reject) => {
      const pedido = f(bd.transaction(LOJA, modo).objectStore(LOJA))
      pedido.onsuccess = () => resolve(pedido.result)
      pedido.onerror = () => reject(pedido.error ?? new Error('falhou'))
    })
  } finally {
    bd.close()
  }
}

export function guardarEfeito(e: EfeitoGuardado): Promise<unknown> {
  return transaccao('readwrite', (l) => l.put(e))
}

export function apagarEfeito(id: string): Promise<unknown> {
  return transaccao('readwrite', (l) => l.delete(id))
}

/**
 * Lê os efeitos guardados. Sem armazenamento (janela privada, quota cheia) não
 * atira: devolve vazio, e o painel mostra as teclas livres. Um estúdio não
 * pode deixar de abrir porque o IndexedDB não está disponível.
 */
export async function lerEfeitos(): Promise<EfeitoGuardado[]> {
  try {
    const todos = await transaccao<EfeitoGuardado[]>('readonly', (l) => l.getAll() as IDBRequest<EfeitoGuardado[]>)
    return todos.filter((e) => indiceDoEfeito(e.id) !== null)
  } catch {
    return []
  }
}
