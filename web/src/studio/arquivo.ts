/**
 * Arquivo local das aulas — IndexedDB.
 *
 * PORQUE EXISTE: sem isto, «funciona offline» era meia verdade. A app abria
 * sem rede e gravava, mas a aula ficava num `Blob` em memória: fechar o
 * separador, ou o browser matar a página por falta de memória, e o trabalho
 * desaparecia sem aviso. Gravar uma aula de quarenta minutos e perdê-la ao
 * fechar por engano é o pior fim possível para esta ferramenta.
 *
 * PORQUE INDEXEDDB E NÃO localStorage: o localStorage guarda texto e anda pelos
 * 5 MB. Uma aula são dezenas ou centenas de MB de binário. O IndexedDB guarda
 * `Blob` nativamente, sem base64 — que inflaria tudo em mais um terço.
 *
 * O QUE ISTO NÃO É: um substituto da biblioteca no servidor. É a fila de espera
 * até haver rede. Quem grava offline vê a aula listada como «por enviar», e o
 * envio acontece sozinho quando a ligação voltar.
 */

const BD = 'delonix-estudio'
const LOJA = 'aulas'
const VERSAO = 1

export interface AulaGuardada {
  id: string
  titulo: string
  criadaEm: number
  duracao: number
  /** `null` quando já foi enviada — o blob é largado para não ocupar disco. */
  completo: Blob | null
  audio: Blob | null
  /** `true` depois de entrar na biblioteca do servidor. */
  enviada: boolean
  /** Última falha de envio, para a interface poder dizer porquê. */
  erro?: string
}

function abrir(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const pedido = indexedDB.open(BD, VERSAO)
    pedido.onupgradeneeded = () => {
      const bd = pedido.result
      if (!bd.objectStoreNames.contains(LOJA)) {
        const loja = bd.createObjectStore(LOJA, { keyPath: 'id' })
        loja.createIndex('enviada', 'enviada', { unique: false })
      }
    }
    pedido.onsuccess = () => resolve(pedido.result)
    pedido.onerror = () => reject(pedido.error ?? new Error('IndexedDB indisponível'))
  })
}

function transacao<T>(modo: IDBTransactionMode, fn: (loja: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return abrir().then(
    (bd) =>
      new Promise<T>((resolve, reject) => {
        const t = bd.transaction(LOJA, modo)
        const pedido = fn(t.objectStore(LOJA))
        pedido.onsuccess = () => resolve(pedido.result)
        pedido.onerror = () => reject(pedido.error ?? new Error('falha no arquivo local'))
        t.oncomplete = () => bd.close()
      }),
  )
}

/** Guarda (ou substitui) uma aula. Devolve o id. */
export async function guardar(aula: Omit<AulaGuardada, 'id'> & { id?: string }): Promise<string> {
  // `crypto.randomUUID` só existe em contexto seguro — que é onde a app corre;
  // o recurso evita rebentar num http:// de rede interna.
  const id = aula.id ?? (crypto.randomUUID?.() ?? `aula-${Date.now()}-${Math.random().toString(36).slice(2)}`)
  await transacao('readwrite', (loja) => loja.put({ ...aula, id }))
  return id
}

export function listar(): Promise<AulaGuardada[]> {
  return transacao<AulaGuardada[]>('readonly', (loja) => loja.getAll() as IDBRequest<AulaGuardada[]>).then((as) =>
    as.sort((a, b) => b.criadaEm - a.criadaEm),
  )
}

export function porEnviar(): Promise<AulaGuardada[]> {
  return listar().then((as) => as.filter((a) => !a.enviada && a.completo))
}

export function apagar(id: string): Promise<void> {
  return transacao('readwrite', (loja) => loja.delete(id)).then(() => undefined)
}

/**
 * Marca como enviada e LARGA os blobs.
 *
 * Guardar a aula depois de ela estar no servidor é ocupar o disco do
 * utilizador duas vezes pela mesma coisa. O registo fica, para a lista poder
 * mostrar o que já foi, mas sem os bytes.
 */
export async function marcarEnviada(id: string): Promise<void> {
  const aula = await transacao<AulaGuardada | undefined>('readonly', (l) => l.get(id) as IDBRequest<AulaGuardada | undefined>)
  if (!aula) return
  await transacao('readwrite', (l) => l.put({ ...aula, enviada: true, completo: null, audio: null, erro: undefined }))
}

export async function marcarErro(id: string, erro: string): Promise<void> {
  const aula = await transacao<AulaGuardada | undefined>('readonly', (l) => l.get(id) as IDBRequest<AulaGuardada | undefined>)
  if (!aula) return
  await transacao('readwrite', (l) => l.put({ ...aula, erro }))
}

/** Espaço ocupado pelas aulas ainda por enviar, em bytes. */
export function ocupacao(): Promise<number> {
  return listar().then((as) => as.reduce((n, a) => n + (a.completo?.size ?? 0) + (a.audio?.size ?? 0), 0))
}
