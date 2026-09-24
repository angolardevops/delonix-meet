/**
 * Onde o modelo editável vive: IndexedDB deste browser, base
 * `delonix-diagramas`, armazém `docs` (chave `id`).
 *
 * Não é o servidor: um diagrama feito aqui não aparece noutro computador até
 * existir `doc JSONB` em `whiteboards` (lote 2). Para o levar, exporta-se o
 * JSON e importa-se do outro lado. A interface diz isto à pessoa.
 */
import type { DiagramDoc } from './model'

const DB = 'delonix-diagramas'
const STORE = 'docs'

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('indexeddb-indisponivel'))
      return
    }
    const req = indexedDB.open(DB, 1)
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains(STORE)) {
        const s = db.createObjectStore(STORE, { keyPath: 'id' })
        s.createIndex('updatedAt', 'updatedAt')
      }
    }
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error ?? new Error('indexeddb'))
    req.onblocked = () => reject(new Error('indexeddb-bloqueada'))
  })
}

async function run<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await open()
  try {
    return await new Promise<T>((resolve, reject) => {
      const tx = db.transaction(STORE, mode)
      const req = fn(tx.objectStore(STORE))
      tx.oncomplete = () => resolve(req.result)
      tx.onerror = () => reject(tx.error ?? req.error ?? new Error('indexeddb'))
      tx.onabort = () => reject(tx.error ?? new Error('indexeddb-abortada'))
    })
  } finally {
    db.close()
  }
}

/** Resumo para a lista (sem carregar elementos de todos). */
export interface DiagramSummary {
  id: string
  title: string
  notation: DiagramDoc['notation']
  roomCode: string
  updatedAt: string
  elements: number
  boardId?: string
  savedAt?: string
}

export async function listDiagrams(): Promise<DiagramSummary[]> {
  const all = await run<DiagramDoc[]>('readonly', (s) => s.getAll() as IDBRequest<DiagramDoc[]>)
  return all
    .map((d) => ({
      id: d.id,
      title: d.title,
      notation: d.notation,
      roomCode: d.roomCode,
      updatedAt: d.updatedAt,
      elements: d.nodes.length + d.strokes.length,
      boardId: d.boardId,
      savedAt: d.savedAt,
    }))
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
}

export async function getDiagram(id: string): Promise<DiagramDoc | null> {
  const d = await run<DiagramDoc | undefined>('readonly', (s) => s.get(id) as IDBRequest<DiagramDoc | undefined>)
  return d ?? null
}

export async function putDiagram(doc: DiagramDoc): Promise<void> {
  await run('readwrite', (s) => s.put(doc))
}

export async function deleteDiagram(id: string): Promise<void> {
  await run('readwrite', (s) => s.delete(id))
}
