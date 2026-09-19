/**
 * Lógica sem UI da entrada e das páginas públicas. Vive fora dos `.tsx` para
 * se poder testar sem browser — é aqui que estão as decisões que já falharam
 * (o domínio do SSO, o formato do código de recuperação, o erro que não pode
 * dizer se a conta existe).
 */

/**
 * Domínio do email, ou `null` quando ainda não há um domínio que valha a pena
 * perguntar ao servidor. Sem ponto não há domínio real: evita um pedido de SSO
 * por cada letra escrita depois da arroba.
 */
export function dominioDoEmail(email: string): string | null {
  const at = email.trim().lastIndexOf('@')
  if (at < 1) return null
  const d = email.trim().slice(at + 1).toLowerCase()
  if (!/^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(d)) return null
  return d
}

/** Os dois códigos que o desafio de segundo factor aceita. */
export type TipoCodigo = 'totp' | 'recuperacao'

/**
 * Normaliza o que a pessoa escreveu no desafio. O TOTP são 6 dígitos; o de
 * recuperação é `XXXXX-XXXXX` em base32 (sem 0/1/8/9) — chega escrito à mão,
 * muitas vezes em minúsculas e sem o hífen.
 */
export function normalizarCodigo(raw: string, tipo: TipoCodigo): string {
  if (tipo === 'totp') return raw.replace(/\D/g, '').slice(0, 6)
  const s = raw.toUpperCase().replace(/[^A-Z2-7]/g, '').slice(0, 10)
  return s.length > 5 ? `${s.slice(0, 5)}-${s.slice(5)}` : s
}

/** O código já tem o formato completo? Só então se envia. */
export function codigoCompleto(code: string, tipo: TipoCodigo): boolean {
  return tipo === 'totp' ? /^\d{6}$/.test(code) : /^[A-Z2-7]{5}-[A-Z2-7]{5}$/.test(code)
}

/**
 * Motivo de uma entrada recusada, como chave de tradução — ou `null` quando a
 * mensagem do próprio servidor é a melhor explicação (400 com texto legível).
 *
 * O 401 é deliberadamente UMA mensagem para «email desconhecido» e «palavra-
 * passe errada»: o servidor já iguala os tempos para não revelar se a conta
 * existe, e o ecrã não pode desfazer isso.
 */
export function motivoDaRecusa(e: unknown): string | null {
  // Pelo nome e não por `instanceof`: este módulo não importa o `api.ts`, que
  // lê o localStorage ao carregar e não corre fora do browser.
  const status = estadoHttp(e)
  if (status !== null) {
    if (status === 401) return 'auth.erro.credenciais'
    if (status === 429) return 'auth.erro.demasiadasTentativas'
    if (status === 409) return 'auth.erro.jaExiste'
    if (status >= 500) return 'auth.erro.servidor'
    return null
  }
  // `fetch` rejeita com TypeError quando não chega ao servidor.
  return 'auth.erro.semLigacao'
}

/** Estado HTTP de um `ApiError`, ou `null` quando o erro não veio do servidor. */
export function estadoHttp(e: unknown): number | null {
  const x = e as { name?: string; status?: unknown } | null
  return x?.name === 'ApiError' && typeof x.status === 'number' ? x.status : null
}

/** Tempo de actividade em partes — o ecrã formata com `t()`. */
export function partesUptime(segundos: number): { d: number; h: number; m: number } {
  const s = Math.max(0, Math.floor(segundos))
  return { d: Math.floor(s / 86400), h: Math.floor((s % 86400) / 3600), m: Math.floor((s % 3600) / 60) }
}

/** Resposta de GET /api/status (server/src/main.rs `status`). */
export interface EstadoServico {
  status: string
  api: boolean
  db: boolean
  uptime_secs: number
  version: string
}

export type Saude = 'operacional' | 'degradado' | 'indisponivel'

/** Estado global a partir da resposta; sem resposta é indisponível. */
export function saudeGlobal(info: EstadoServico | null): Saude {
  if (!info) return 'indisponivel'
  return info.status === 'ok' && info.api && info.db ? 'operacional' : 'degradado'
}

/** Tamanho legível em MB com uma casa, como a gravação é descrita no servidor. */
export function megabytes(bytes: number): string {
  return (bytes / 1_048_576).toFixed(1)
}
