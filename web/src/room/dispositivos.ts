/**
 * «Estás também ao telefone»: o estado das perguntas, sem React.
 *
 * O servidor manda um aviso por perna de telefone (e volta a mandá-los a cada
 * entrada no browser: F5, reconexão). Aqui decide-se quais mostrar: uma fila por
 * `phone_id`, sem repetidos, sem os que a pessoa já decidiu nesta sala.
 */
export interface AvisoDispositivo {
  phoneId: string
  canHangup: boolean
}

export type ResultadoDispositivo = 'hung_up' | 'muted' | 'both' | 'gone'

const chave = (code: string) => `dx_dup_${code}`

/** As pernas que a pessoa já decidiu (ou dispensou) nesta sala, deste separador. */
export function decididas(code: string, storage: Pick<Storage, 'getItem'> | null = safeSession()): Set<string> {
  try {
    const raw = storage?.getItem(chave(code))
    const l = raw ? (JSON.parse(raw) as unknown) : []
    return new Set(Array.isArray(l) ? l.filter((x): x is string => typeof x === 'string') : [])
  } catch {
    return new Set()
  }
}

export function lembrar(code: string, phoneId: string, storage: Pick<Storage, 'getItem' | 'setItem'> | null = safeSession()) {
  try {
    const s = decididas(code, storage)
    s.add(phoneId)
    storage?.setItem(chave(code), JSON.stringify([...s].slice(-20)))
  } catch {
    /* sem storage (modo privado): pergunta-se outra vez, nada parte */
  }
}

function safeSession(): Storage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage
  } catch {
    return null
  }
}

/** Acrescenta um aviso à fila: ignora o já decidido e o já na fila (actualiza-o). */
export function enfileirar(fila: AvisoDispositivo[], aviso: AvisoDispositivo, jaDecididas: Set<string>): AvisoDispositivo[] {
  if (jaDecididas.has(aviso.phoneId)) return fila
  const i = fila.findIndex((a) => a.phoneId === aviso.phoneId)
  if (i >= 0) return fila.map((a, k) => (k === i ? aviso : a))
  return [...fila, aviso]
}

/** Tira da fila só a perna resolvida: as outras perguntas continuam à espera. */
export function resolver(fila: AvisoDispositivo[], phoneId: string): AvisoDispositivo[] {
  return fila.filter((a) => a.phoneId !== phoneId)
}
