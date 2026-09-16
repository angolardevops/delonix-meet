/**
 * Projectos de edição no dispositivo — IndexedDB `delonix-editor`.
 *
 * Base PRÓPRIA, não uma loja nova na `delonix-estudio` do `arquivo.ts`: mudar
 * a versão dessa base obrigava a coordenar com quem já a abre (o arquivo de
 * aulas por enviar), e um `onupgradeneeded` esquecido deixava a fila offline
 * sem abrir. Aqui a versão é nossa.
 *
 * Três lojas:
 *  - `projectos`: o JSON do projecto (a lista de edições) e o histórico de
 *    desfazer — pequeno, reescrito a cada gravação automática;
 *  - `fontes`: os BLOBS, escritos UMA vez e nunca alterados (é isso que faz o
 *    projecto não destrutivo);
 *  - `exportacoes`: o histórico das exportações feitas NESTE browser (sem os
 *    ficheiros — esses foram descarregados ou enviados).
 */
import type { Projecto } from './projecto'

const BD = 'delonix-editor'
const VERSAO = 1

export interface RegistoDeProjecto {
  id: string
  projecto: Projecto
  /** Estados anteriores, para o desfazer sobreviver a recarregar a página. */
  passado: Projecto[]
  futuro: Projecto[]
  alteradoEm: number
}

export interface RegistoDeFonte {
  id: string
  projectoId: string
  blob: Blob
}

export interface RegistoDeExportacao {
  id: string
  projectoId: string
  titulo: string
  predefinicao: string
  formato: string
  bytes: number
  duracao: number
  destino: 'descarregado' | 'biblioteca'
  estado: 'concluida' | 'falhou' | 'cancelada'
  erro?: string
  criadaEm: number
  segundos: number
}

let aberta: Promise<IDBDatabase> | null = null

function abrir(): Promise<IDBDatabase> {
  aberta ??= new Promise((resolve, reject) => {
    const pedido = indexedDB.open(BD, VERSAO)
    pedido.onupgradeneeded = () => {
      const bd = pedido.result
      if (!bd.objectStoreNames.contains('projectos')) bd.createObjectStore('projectos', { keyPath: 'id' })
      if (!bd.objectStoreNames.contains('fontes')) {
        const f = bd.createObjectStore('fontes', { keyPath: 'id' })
        f.createIndex('projectoId', 'projectoId', { unique: false })
      }
      if (!bd.objectStoreNames.contains('exportacoes')) bd.createObjectStore('exportacoes', { keyPath: 'id' })
    }
    pedido.onsuccess = () => {
      const bd = pedido.result
      // Outra aba a subir a versão: larga-se a ligação para não a bloquear.
      bd.onversionchange = () => {
        bd.close()
        aberta = null
      }
      resolve(bd)
    }
    pedido.onerror = () => {
      aberta = null
      reject(pedido.error ?? new Error('IndexedDB indisponível'))
    }
  })
  return aberta
}

function pedido<T>(loja: string, modo: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return abrir().then(
    (bd) =>
      new Promise<T>((resolve, reject) => {
        const t = bd.transaction(loja, modo)
        const r = fn(t.objectStore(loja))
        let valor: T
        r.onsuccess = () => {
          valor = r.result
        }
        t.oncomplete = () => resolve(valor)
        t.onerror = () => reject(t.error ?? r.error ?? new Error('falha no IndexedDB'))
        t.onabort = () => reject(t.error ?? new Error('transacção abortada'))
      }),
  )
}

export function guardarProjecto(r: RegistoDeProjecto): Promise<void> {
  // O histórico guardado é curto: 30 passos chegam para recuperar de um
  // engano depois de recarregar, e cada passo é um projecto inteiro.
  const reg = { ...r, passado: r.passado.slice(-30), futuro: r.futuro.slice(0, 30) }
  return pedido('projectos', 'readwrite', (s) => s.put(reg)).then(() => undefined)
}

export function lerProjecto(id: string): Promise<RegistoDeProjecto | undefined> {
  return pedido<RegistoDeProjecto | undefined>('projectos', 'readonly', (s) => s.get(id) as IDBRequest<RegistoDeProjecto | undefined>)
}

export async function listarProjectos(): Promise<RegistoDeProjecto[]> {
  const todos = await pedido<RegistoDeProjecto[]>('projectos', 'readonly', (s) => s.getAll() as IDBRequest<RegistoDeProjecto[]>)
  return todos.sort((a, b) => b.alteradoEm - a.alteradoEm)
}

export async function apagarProjecto(id: string): Promise<void> {
  const bd = await abrir()
  await new Promise<void>((resolve, reject) => {
    const t = bd.transaction(['projectos', 'fontes'], 'readwrite')
    t.objectStore('projectos').delete(id)
    const idx = t.objectStore('fontes').index('projectoId').openCursor(IDBKeyRange.only(id))
    idx.onsuccess = () => {
      const c = idx.result
      if (c) {
        c.delete()
        c.continue()
      }
    }
    t.oncomplete = () => resolve()
    t.onerror = () => reject(t.error ?? new Error('falha ao apagar'))
  })
}

export function guardarFonte(r: RegistoDeFonte): Promise<void> {
  return pedido('fontes', 'readwrite', (s) => s.put(r)).then(() => undefined)
}

export async function lerFonte(id: string): Promise<Blob | null> {
  const r = await pedido<RegistoDeFonte | undefined>('fontes', 'readonly', (s) => s.get(id) as IDBRequest<RegistoDeFonte | undefined>)
  return r?.blob ?? null
}

export function apagarFonte(id: string): Promise<void> {
  return pedido('fontes', 'readwrite', (s) => s.delete(id)).then(() => undefined)
}

export function registarExportacao(r: RegistoDeExportacao): Promise<void> {
  return pedido('exportacoes', 'readwrite', (s) => s.put(r)).then(() => undefined)
}

export async function listarExportacoes(): Promise<RegistoDeExportacao[]> {
  const todos = await pedido<RegistoDeExportacao[]>('exportacoes', 'readonly', (s) => s.getAll() as IDBRequest<RegistoDeExportacao[]>)
  return todos.sort((a, b) => b.criadaEm - a.criadaEm)
}

export function limparExportacoes(): Promise<void> {
  return pedido('exportacoes', 'readwrite', (s) => s.clear()).then(() => undefined)
}

/** Espaço ocupado pelos blobs de um projecto. */
export async function bytesDoProjecto(p: Projecto): Promise<number> {
  return p.fontes.reduce((n, f) => n + f.bytes, 0)
}
