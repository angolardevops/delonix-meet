/**
 * O chat da sala como DADOS: histórico do servidor, mensagens ao vivo, o eco
 * do que eu enviei (`chat-sent`), respostas e reacções. Puro, para se testar
 * sem socket — o `useChat` só liga isto às mensagens.
 *
 * O servidor é a fonte do `id` e da hora (`at`). Uma mensagem minha nasce
 * `pending` com a hora deste dispositivo e passa a ter os do servidor quando
 * chega o `chat-sent` com o mesmo `client_id`.
 */
export interface ChatMsg {
  /** Chave estável para a lista (não muda quando chega o id do servidor). */
  key: string
  /** Id do servidor; `null` enquanto não confirmada. */
  id: string | null
  clientId?: string
  username: string
  text: string
  own: boolean
  historical?: boolean
  /** Hora do servidor (ms); até confirmar, a deste dispositivo. */
  at: number
  /** peer_id de quem enviou (ao vivo) — para o papel na mensagem. */
  from?: string
  /** Id da mensagem a que responde. */
  replyTo: string | null
  /** Contagens por emoji, sempre as do servidor. */
  reactions: Record<string, number>
  pending?: boolean
  /** Conversa directa: só eu e a outra pessoa a vemos. */
  private?: boolean
  /** Privada: `peer_id` de quem a recebe (ao vivo). */
  to?: string | null
  /** Privada: nome de quem a recebe. */
  toUsername?: string | null
}

export interface HistoricoChat {
  id: string
  user_id: string
  username: string
  message: string
  created_at: string
  parent_id?: string | null
  reactions?: Record<string, number> | null
  to_user_id?: string | null
  to_username?: string | null
}

let seq = 0
const chave = () => `m${++seq}`

/** Junta o histórico com o que já chegou ao vivo, sem duplicar ids. */
export function comHistorico(actual: ChatMsg[], historico: HistoricoChat[], meuId: string | undefined): ChatMsg[] {
  const vivos = actual.filter((m) => !m.historical)
  const idsVivos = new Set(vivos.map((m) => m.id).filter(Boolean))
  const antigos: ChatMsg[] = historico
    .filter((h) => !idsVivos.has(h.id))
    .map((h) => ({
      key: `h${h.id}`,
      id: h.id,
      username: h.username,
      text: h.message,
      own: h.user_id === meuId,
      historical: true,
      at: Date.parse(h.created_at) || Date.now(),
      replyTo: h.parent_id ?? null,
      reactions: h.reactions ?? {},
      private: !!h.to_user_id,
      toUsername: h.to_username ?? null,
    }))
    .sort((a, b) => a.at - b.at)
  return [...antigos, ...vivos]
}

/** Mensagem de outra pessoa (ou o eco da minha, se o servidor o mandar). */
export function comRecebida(
  actual: ChatMsg[],
  m: { from: string; username: string; text: string; id?: string; at?: number; reply_to?: string | null; to?: string | null; to_username?: string | null },
): ChatMsg[] {
  if (m.id && actual.some((x) => x.id === m.id)) return actual
  return [
    ...actual,
    {
      key: chave(),
      id: m.id ?? null,
      username: m.username,
      text: m.text,
      own: false,
      at: m.at ?? Date.now(),
      from: m.from,
      replyTo: m.reply_to ?? null,
      reactions: {},
      private: !!m.to,
      to: m.to ?? null,
      toUsername: m.to_username ?? null,
    },
  ]
}

/** A minha, antes da confirmação. */
export function comEnviada(
  actual: ChatMsg[],
  m: { clientId: string; username: string; text: string; replyTo: string | null; at: number; to?: string | null; toUsername?: string | null },
): ChatMsg[] {
  return [
    ...actual,
    {
      key: chave(),
      id: null,
      clientId: m.clientId,
      username: m.username,
      text: m.text,
      own: true,
      at: m.at,
      replyTo: m.replyTo,
      reactions: {},
      pending: true,
      private: !!m.to,
      to: m.to ?? null,
      toUsername: m.toUsername ?? null,
    },
  ]
}

/** `chat-sent`: a minha mensagem ganha id e hora do servidor. */
export function comConfirmada(actual: ChatMsg[], m: { client_id: string; id: string; at: number }): ChatMsg[] {
  return actual.map((x) => (x.clientId === m.client_id ? { ...x, id: m.id, at: m.at, pending: false } : x))
}

/** `chat-reactions`: o estado COMPLETO das reacções de uma mensagem. */
export function comReaccoes(actual: ChatMsg[], m: { id: string; counts: Record<string, number> }): ChatMsg[] {
  const limpo = Object.fromEntries(Object.entries(m.counts ?? {}).filter(([, n]) => n > 0))
  return actual.map((x) => (x.id === m.id ? { ...x, reactions: limpo } : x))
}

/** Respostas por mensagem-mãe, pela ordem de chegada. */
export function respostasPorMae(msgs: ChatMsg[]): Map<string, ChatMsg[]> {
  const out = new Map<string, ChatMsg[]>()
  for (const m of msgs) {
    if (!m.replyTo) continue
    const l = out.get(m.replyTo) ?? []
    l.push(m)
    out.set(m.replyTo, l)
  }
  return out
}
