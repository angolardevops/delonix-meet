import { currentUser } from './api'

/**
 * O ESTADO INICIAL com que a sala nasce para este utilizador — não o que
 * está ligado AGORA. Mudar microfone/câmara/fundo a meio de uma chamada
 * (`useLocalMedia`) fica só nessa sessão; só as Preferências (Definições ➜
 * Conta) escrevem aqui, de propósito — sem isso, qualquer toggle de
 * emergência a meio de uma reunião viraria o teu omissão para sempre.
 */
export interface JoinPrefs {
  noiseSuppression: boolean
  blurByDefault: boolean
}

const DEFAULT_JOIN_PREFS: JoinPrefs = { noiseSuppression: true, blurByDefault: false }

const key = (userId: string) => `dx_join_prefs:${userId}`

export function readJoinPrefs(): JoinPrefs {
  const userId = currentUser()?.id
  if (!userId) return DEFAULT_JOIN_PREFS
  try {
    const raw = localStorage.getItem(key(userId))
    if (!raw) return DEFAULT_JOIN_PREFS
    return { ...DEFAULT_JOIN_PREFS, ...JSON.parse(raw) }
  } catch {
    return DEFAULT_JOIN_PREFS
  }
}

export function writeJoinPrefs(patch: Partial<JoinPrefs>) {
  const userId = currentUser()?.id
  if (!userId) return
  try {
    localStorage.setItem(key(userId), JSON.stringify({ ...readJoinPrefs(), ...patch }))
  } catch {
    /* sem armazenamento: vale só para esta sessão */
  }
}
