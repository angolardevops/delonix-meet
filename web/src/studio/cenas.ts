/**
 * Banco de cenas do Estúdio — IndexedDB.
 *
 * Uma CENA é uma composição que se reaplica com um clique: o layout, o que
 * enche o palco (fontes, cartão de intervalo, quadro), a bolha da câmara e as
 * sobreposições. NÃO guarda fontes: o ecrã, a câmara e os convidados são da
 * sessão, e uma cena que prometesse «o ecrã de ontem» estaria a mentir.
 *
 * PORQUE UMA BASE PRÓPRIA (`delonix-estudio-palco`) e não uma loja nova na
 * `delonix-estudio` do `arquivo.ts`: a versão de uma base IndexedDB é UMA só.
 * Se duas equipas acrescentarem lojas à mesma base em ramos diferentes, as duas
 * sobem para a versão 2 — e quem abrir primeiro uma delas nunca recebe a loja
 * da outra, porque o `onupgradeneeded` já não volta a correr. Bases separadas
 * não têm essa corrida.
 *
 * O `logotipo` (imagem carregada pela pessoa) vive na loja `meta` da mesma
 * base: é um Blob, e o localStorage não guarda binário.
 */
import type { EstadoDoAvatar } from './compositor'
import type { ConteudoDoPalco, LayoutDoPalco, Sobreposicoes } from './palco'

const BD = 'delonix-estudio-palco'
const VERSAO = 1
const CENAS = 'cenas'
const META = 'meta'

export interface CenaGuardada {
  id: string
  nome: string
  criadaEm: number
  layout: LayoutDoPalco
  conteudo: ConteudoDoPalco
  avatar: EstadoDoAvatar
  sobreposicoes: Sobreposicoes
  /** Miniatura JPEG do palco no momento em que foi guardada. */
  miniatura: Blob | null
}

function abrir(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const pedido = indexedDB.open(BD, VERSAO)
    pedido.onupgradeneeded = () => {
      const bd = pedido.result
      if (!bd.objectStoreNames.contains(CENAS)) bd.createObjectStore(CENAS, { keyPath: 'id' })
      if (!bd.objectStoreNames.contains(META)) bd.createObjectStore(META)
    }
    pedido.onsuccess = () => resolve(pedido.result)
    pedido.onerror = () => reject(pedido.error ?? new Error('IndexedDB indisponível'))
  })
}

function transacao<T>(loja: string, modo: IDBTransactionMode, fn: (l: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return abrir().then(
    (bd) =>
      new Promise<T>((resolve, reject) => {
        const t = bd.transaction(loja, modo)
        const pedido = fn(t.objectStore(loja))
        pedido.onsuccess = () => resolve(pedido.result)
        pedido.onerror = () => reject(pedido.error ?? new Error('falha no banco de cenas'))
        t.oncomplete = () => bd.close()
      }),
  )
}

const novoId = () => crypto.randomUUID?.() ?? `cena-${Date.now()}-${Math.random().toString(36).slice(2)}`

export function listarCenas(): Promise<CenaGuardada[]> {
  return transacao<CenaGuardada[]>(CENAS, 'readonly', (l) => l.getAll() as IDBRequest<CenaGuardada[]>).then((cs) =>
    cs.sort((a, b) => a.criadaEm - b.criadaEm),
  )
}

export async function guardarCena(cena: Omit<CenaGuardada, 'id' | 'criadaEm'> & { id?: string; criadaEm?: number }) {
  const completa: CenaGuardada = { ...cena, id: cena.id ?? novoId(), criadaEm: cena.criadaEm ?? Date.now() }
  await transacao(CENAS, 'readwrite', (l) => l.put(completa))
  return completa
}

export function apagarCena(id: string): Promise<void> {
  return transacao(CENAS, 'readwrite', (l) => l.delete(id)).then(() => undefined)
}

/**
 * Semeia as cenas de partida UMA vez por dispositivo. Cada uma é uma
 * composição que funciona de facto (nenhuma aponta para uma fonte que não
 * existe) — são atalhos, não decoração. Apagá-las todas não as faz voltar.
 */
let semeando: Promise<boolean> | null = null

export function semearSeVazio(iniciais: Omit<CenaGuardada, 'id' | 'criadaEm'>[]): Promise<boolean> {
  // Uma só sementeira por carregamento: o React monta os efeitos duas vezes em
  // desenvolvimento, e duas leituras de «ainda não semeado» davam oito cenas.
  semeando ??= semear(iniciais).catch((e) => {
    semeando = null
    throw e
  })
  return semeando
}

async function semear(iniciais: Omit<CenaGuardada, 'id' | 'criadaEm'>[]): Promise<boolean> {
  const ja = await transacao<unknown>(META, 'readonly', (l) => l.get('semeadas') as IDBRequest<unknown>)
  if (ja) return false
  const agora = Date.now()
  for (const [i, c] of iniciais.entries()) await guardarCena({ ...c, criadaEm: agora + i })
  await transacao(META, 'readwrite', (l) => l.put(true, 'semeadas'))
  return true
}

export function lerLogotipo(): Promise<Blob | null> {
  return transacao<Blob | undefined>(META, 'readonly', (l) => l.get('logotipo') as IDBRequest<Blob | undefined>).then(
    (b) => b ?? null,
  )
}

export function guardarLogotipo(b: Blob | null): Promise<void> {
  return transacao<unknown>(META, 'readwrite', (l) =>
    b ? (l.put(b, 'logotipo') as IDBRequest<unknown>) : (l.delete('logotipo') as IDBRequest<unknown>),
  ).then(() => undefined)
}

/** Uma miniatura 320×180 do palco — o suficiente para reconhecer a cena. */
export function miniaturaDe(canvas: HTMLCanvasElement): Promise<Blob | null> {
  const m = document.createElement('canvas')
  m.width = 320
  m.height = 180
  m.getContext('2d')?.drawImage(canvas, 0, 0, 320, 180)
  return new Promise((res) => m.toBlob((b) => res(b), 'image/jpeg', 0.7))
}
