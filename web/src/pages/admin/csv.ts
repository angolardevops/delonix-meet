/**
 * Leitura de CSV sem UI (para se poder testar sem browser) para a
 * importação em massa de convites. Não é RFC 4180 completa — só o
 * suficiente para um CSV escrito ou exportado por um admin: vírgulas,
 * campos entre aspas com vírgulas lá dentro, e aspas escapadas a dobrar
 * (`""`). Não se traz uma dependência nova só para isto.
 */

export interface CsvInviteRow {
  email: string
  title?: string
  role?: string
  branch?: string
}

export interface CsvParseResult {
  rows: CsvInviteRow[]
  /** Chave de tradução do erro, se o CSV não tiver uma coluna `email`. */
  errorKey?: string
}

function parseCsvLine(line: string): string[] {
  const out: string[] = []
  let cur = ''
  let inQuotes = false
  for (let i = 0; i < line.length; i++) {
    const c = line[i]
    if (inQuotes) {
      if (c === '"') {
        if (line[i + 1] === '"') {
          cur += '"'
          i++
        } else {
          inQuotes = false
        }
      } else {
        cur += c
      }
    } else if (c === '"') {
      inQuotes = true
    } else if (c === ',') {
      out.push(cur)
      cur = ''
    } else {
      cur += c
    }
  }
  out.push(cur)
  return out
}

/** Linhas não vazias, cada uma já separada em campos. */
export function parseCsvLines(text: string): string[][] {
  return text
    .split(/\r\n|\n|\r/)
    .filter((l) => l.trim() !== '')
    .map(parseCsvLine)
}

/**
 * Header obrigatório: `email`. `title`/`role`/`branch` são opcionais, em
 * qualquer ordem, sem distinguir maiúsculas/minúsculas — e colunas extra
 * (ex.: exportadas de outro sítio) são ignoradas em vez de dar erro.
 */
export function parseInviteCsv(text: string): CsvParseResult {
  const lines = parseCsvLines(text)
  if (lines.length === 0) return { rows: [], errorKey: 'org.csv.erroVazio' }
  const header = lines[0].map((h) => h.trim().toLowerCase())
  const emailIdx = header.indexOf('email')
  if (emailIdx === -1) return { rows: [], errorKey: 'org.csv.erroSemEmail' }
  const titleIdx = header.indexOf('title')
  const roleIdx = header.indexOf('role')
  const branchIdx = header.indexOf('branch')

  const rows = lines
    .slice(1)
    .map((cols) => ({
      email: (cols[emailIdx] ?? '').trim(),
      title: titleIdx >= 0 ? cols[titleIdx]?.trim() || undefined : undefined,
      role: roleIdx >= 0 ? cols[roleIdx]?.trim() || undefined : undefined,
      branch: branchIdx >= 0 ? cols[branchIdx]?.trim() || undefined : undefined,
    }))
    .filter((r) => r.email !== '')
  return { rows }
}
