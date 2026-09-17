// Utilitário de teste partilhado: verificação de XML bem formado sem DOM.
/**
 * Verificação de XML bem formado sem DOM: etiquetas equilibradas, atributos
 * entre aspas, entidades conhecidas e ids únicos. Não substitui um parser,
 * mas apanha exactamente o que um exportador por concatenação costuma partir.
 */
export function wellFormed(xml: string): { ok: boolean; why?: string; ids: string[] } {
  const body = xml.replace(/^<\?xml[^?]*\?>\s*/, '')
  const stack: string[] = []
  const ids: string[] = []
  const tag = /<(\/?)([A-Za-z_][\w:.-]*)((?:\s+[\w:.-]+="[^"<]*")*)\s*(\/?)>|([^<]+)/gy
  let m: RegExpExecArray | null
  let pos = 0
  while (pos < body.length) {
    tag.lastIndex = pos
    m = tag.exec(body)
    if (!m) return { ok: false, why: `lixo em ${pos}: ${body.slice(pos, pos + 40)}`, ids }
    pos = tag.lastIndex
    if (m[5] !== undefined) {
      if (/&(?!amp;|lt;|gt;|quot;|apos;)/.test(m[5])) return { ok: false, why: `entidade em texto: ${m[5].slice(0, 40)}`, ids }
      continue
    }
    const [, close, name, attrs, selfClose] = m
    if (/&(?!amp;|lt;|gt;|quot;|apos;)/.test(attrs)) return { ok: false, why: `entidade em atributo de ${name}`, ids }
    for (const a of attrs.matchAll(/\s(?:xmi:)?id="([^"]*)"/g)) ids.push(a[1])
    if (close) {
      const top = stack.pop()
      if (top !== name) return { ok: false, why: `fecha ${name}, aberto ${top}`, ids }
    } else if (!selfClose) stack.push(name)
  }
  if (stack.length) return { ok: false, why: `por fechar: ${stack.join(',')}`, ids }
  return { ok: true, ids }
}

