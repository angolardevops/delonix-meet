/**
 * Onde o modelo editável vive: IndexedDB deste browser, base
 * `delonix-diagramas`, armazém `docs` (chave `id`) — **e agora também o
 * servidor** (`/api/diagrams`, ADR-0020).
 *
 * **O que mudou e porquê.** Até 2026-10-06 isto era só local, e o comentário
 * deste ficheiro dizia o preço: «um diagrama feito aqui não aparece noutro
 * computador». A única forma de o levar era exportar o JSON à mão; mudar de
 * browser, ou limpar os dados do sítio, perdia tudo.
 *
 * **O IndexedDB FICA, e fica primeiro.** Ele é que faz o ecrã responder sem
 * rede e sem espera: toda a leitura começa nele e toda a escrita acaba nele
 * antes de alguém falar com o servidor. O servidor é a segunda ponta, não a
 * primeira — trocar a ordem transformava um editor offline-first num ecrã que
 * roda à espera de rede.
 *
 * **O que isto NÃO é.** Não é edição em simultâneo. Ganha o `updatedAt` mais
 * recente, documento inteiro (`sync.ts`); duas pessoas no mesmo diagrama ao
 * mesmo tempo não é um caso que isto resolva. O caso que resolve é o real: a
 * mesma pessoa noutro computador, ou a voltar depois de estar offline.
 *
 * **Sem sessão não há sincronização, e não é um erro.** Um `401` desliga a
 * segunda ponta em silêncio e o editor continua a funcionar como sempre
 * funcionou.
 */
import {
  ApiError,
  deleteServerDiagram,
  getServerDiagram,
  listServerDiagrams,
  putServerDiagram,
} from '../../api'
import type { DiagramDoc } from './model'
import { compara, planeia } from './sync'

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

// ---------------------------------------------------------------- a 2ª ponta
//
// O `updated_at` que o servidor devolveu da última vez, por id. Vai no `PUT`
// como `expected_updated_at`: é o que faz duas abas receberem `409` em vez de
// escreverem uma por cima da outra em silêncio.
const vistoNoServidor = new Map<string, string>()

/** Sessão ausente (ou expirada): a sincronização desliga-se e não é um erro. */
function semSessao(e: unknown): boolean {
  return e instanceof ApiError && (e.status === 401 || e.status === 403)
}

/**
 * Uma falha de rede não pode partir o editor. Qualquer erro da segunda ponta
 * cai aqui: o trabalho está no IndexedDB, e a próxima gravação tenta outra vez.
 */
function naoImporta(_e: unknown): void {}

function resumoDe(d: DiagramDoc): DiagramSummary {
  return {
    id: d.id,
    title: d.title,
    notation: d.notation,
    roomCode: d.roomCode,
    updatedAt: d.updatedAt,
    elements: d.nodes.length + d.strokes.length,
    boardId: d.boardId,
    savedAt: d.savedAt,
  }
}

function corpoDe(doc: DiagramDoc) {
  const visto = vistoNoServidor.get(doc.id)
  return {
    title: doc.title,
    notation: doc.notation,
    room_code: doc.roomCode,
    elements: doc.nodes.length + doc.strokes.length,
    doc,
    ...(visto ? { expected_updated_at: visto } : {}),
  }
}

/** Envia um diagrama. Um `409` NÃO reescreve nada: quem mudou ganha. */
async function enviar(doc: DiagramDoc): Promise<void> {
  const meta = await putServerDiagram(doc.id, corpoDe(doc))
  vistoNoServidor.set(doc.id, meta.updated_at)
}

/** Traz um diagrama do servidor para o IndexedDB. */
async function descarregar(id: string): Promise<DiagramDoc | null> {
  const r = await getServerDiagram(id)
  vistoNoServidor.set(id, r.updated_at)
  const doc = r.doc as DiagramDoc | null
  // O `doc` é opaco para o servidor: o que volta pode ser qualquer JSON, e
  // escrever lixo no IndexedDB com a chave certa deixava o editor a abrir um
  // documento sem `nodes`.
  if (!doc || typeof doc !== 'object' || !Array.isArray(doc.nodes)) return null
  await run('readwrite', (s) => s.put({ ...doc, id }))
  return { ...doc, id }
}

/**
 * Gravar no servidor é mais lento do que gravar aqui, e a gravação automática
 * do editor corre meio segundo depois de cada tecla. Sem este atraso, arrastar
 * um nó durante dez segundos eram vinte `PUT` de um documento inteiro.
 */
const ENVIO_ATRASO_MS = 3000
const porEnviar = new Map<string, { doc: DiagramDoc; h: ReturnType<typeof setTimeout> }>()

function agendarEnvio(doc: DiagramDoc): void {
  const antes = porEnviar.get(doc.id)
  if (antes) clearTimeout(antes.h)
  const h = setTimeout(() => {
    const pendente = porEnviar.get(doc.id)
    porEnviar.delete(doc.id)
    if (pendente) enviar(pendente.doc).catch(naoImporta)
  }, ENVIO_ATRASO_MS)
  // Coalescido: fica só o ÚLTIMO documento, não uma fila de versões.
  porEnviar.set(doc.id, { doc, h })
}

/**
 * Força o envio do que está agendado. Para quem sai do editor — sem isto, fechar
 * o separador dentro dos três segundos deixava a última alteração só aqui.
 */
export function sincronizaJa(): Promise<void> {
  const tudo = [...porEnviar.values()]
  for (const p of tudo) clearTimeout(p.h)
  porEnviar.clear()
  return Promise.all(tudo.map((p) => enviar(p.doc).catch(naoImporta))).then(() => undefined)
}

// -------------------------------------------------------------- as 4 de antes

/**
 * A lista. Devolve-se **já** o melhor de cada lado (incluindo o que só existe
 * no servidor), e as duas filas de trabalho correm depois — quem abre a lista
 * não espera por uploads.
 */
export async function listDiagrams(): Promise<DiagramSummary[]> {
  const all = await run<DiagramDoc[]>('readonly', (s) => s.getAll() as IDBRequest<DiagramDoc[]>)
  const locais = all.map(resumoDe)
  const porId = new Map(all.map((d) => [d.id, d]))

  let doServidor: DiagramSummary[]
  try {
    const items = await listServerDiagrams()
    doServidor = items.map((i) => {
      vistoNoServidor.set(i.id, i.updated_at)
      return {
        id: i.id,
        title: i.title,
        notation: i.notation as DiagramDoc['notation'],
        roomCode: i.room_code,
        updatedAt: i.updated_at,
        elements: i.elements,
      }
    })
  } catch (e) {
    if (!semSessao(e)) naoImporta(e)
    // Offline ou sem sessão: a lista é a local, como sempre foi.
    return locais.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
  }

  const plano = planeia(locais, doServidor)
  // Em FUNDO, e sem travar a lista: o que falta lá vai, o que falta aqui vem.
  for (const id of plano.enviar) {
    const doc = porId.get(id)
    if (doc) enviar(doc).catch(naoImporta)
  }
  for (const id of plano.descarregar) descarregar(id).catch(naoImporta)
  return plano.lista
}

/**
 * Um diagrama. Começa no IndexedDB (instantâneo) e só troca pelo do servidor se
 * ele for MAIS RECENTE — abrir noutro computador traz o trabalho de ontem, e
 * abrir aqui depois de editar offline não o perde.
 */
export async function getDiagram(id: string): Promise<DiagramDoc | null> {
  const local = (await run<DiagramDoc | undefined>('readonly', (s) => s.get(id) as IDBRequest<DiagramDoc | undefined>)) ?? null

  let remoto: { id: string; updatedAt: string } | undefined
  try {
    const items = await listServerDiagrams()
    const m = items.find((i) => i.id === id)
    if (m) {
      vistoNoServidor.set(id, m.updated_at)
      remoto = { id, updatedAt: m.updated_at }
    }
  } catch (e) {
    if (!semSessao(e)) naoImporta(e)
    return local
  }

  const lado = compara(local ? { id, updatedAt: local.updatedAt } : undefined, remoto)
  if (lado === 'descarregar' || lado === 'so-servidor') {
    try {
      const vindo = await descarregar(id)
      if (vindo) return vindo
    } catch (e) {
      naoImporta(e)
    }
  }
  if (lado === 'so-local' || lado === 'enviar') {
    if (local) agendarEnvio(local)
  }
  return local
}

/**
 * Grava. O IndexedDB é escrito e esperado — é dele que o indicador «guardado»
 * do editor fala; o servidor vai a seguir, atrasado e coalescido.
 */
export async function putDiagram(doc: DiagramDoc): Promise<void> {
  await run('readwrite', (s) => s.put(doc))
  agendarEnvio(doc)
}

/** Apaga nas duas pontas. Se o servidor recusar, o local já foi: o diagrama
 *  volta a aparecer na próxima lista (vem de lá) — melhor do que ficar com um
 *  fantasma que não se consegue apagar. */
export async function deleteDiagram(id: string): Promise<void> {
  const agendado = porEnviar.get(id)
  if (agendado) {
    clearTimeout(agendado.h)
    porEnviar.delete(id)
  }
  await run('readwrite', (s) => s.delete(id))
  vistoNoServidor.delete(id)
  try {
    await deleteServerDiagram(id)
  } catch (e) {
    if (!semSessao(e)) naoImporta(e)
  }
}
