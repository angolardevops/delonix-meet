/**
 * «Guardar chat»: o que ESTE dispositivo recebeu, em texto simples. É uma
 * exportação local — não finge ser o histórico do servidor.
 */
export interface LinhaChat {
  at: number
  username: string
  text: string
}

export interface ItemSondagem {
  at: number
  question: string
  options: string[]
  counts: number[]
}

function hhmm(ms: number, locale: string): string {
  return new Date(ms).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })
}

export function chatEmTexto(
  titulo: string,
  mensagens: LinhaChat[],
  sondagens: ItemSondagem[],
  locale: string,
  rotuloSondagem: string,
): string {
  type Item = { at: number; linhas: string[] }
  const itens: Item[] = [
    ...mensagens.map((m) => ({ at: m.at, linhas: [`[${hhmm(m.at, locale)}] ${m.username}: ${m.text}`] })),
    ...sondagens.map((p) => {
      const total = p.counts.reduce((a, b) => a + b, 0)
      return {
        at: p.at,
        linhas: [
          `[${hhmm(p.at, locale)}] ${rotuloSondagem}: ${p.question}`,
          ...p.options.map((o, i) => `    - ${o}: ${p.counts[i] ?? 0} (${total ? Math.round(((p.counts[i] ?? 0) / total) * 100) : 0}%)`),
        ],
      }
    }),
  ].sort((a, b) => a.at - b.at)
  return [titulo, '', ...itens.flatMap((i) => i.linhas), ''].join('\n')
}

/** Nome de ficheiro seguro: `chat-<sala>-<AAAA-MM-DD>.txt`. */
export function nomeFicheiroChat(code: string, agora: Date): string {
  const d = agora.toISOString().slice(0, 10)
  return `chat-${code.replace(/[^a-z0-9-]/gi, '')}-${d}.txt`
}
