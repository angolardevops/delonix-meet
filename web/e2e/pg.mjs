// MÓDULO DE APOIO — não é um teste; é importado pela auditoria e pelos tempos.
// Descoberta do contentor do Postgres para os testes que atacam a base de
// dados directamente (a auditoria e os tempos).
//
// O nome do contentor vem do nome do DIRECTÓRIO do worktree — `wt-merge-…`,
// `wt-tempo-…`, `wt-audit-…` — por isso um nome fixo num teste só funciona no
// worktree onde ele foi escrito. Isso já custou duas correcções iguais em
// ficheiros diferentes; fica num sítio.
import { execFileSync } from 'node:child_process'

export function contentorPostgres() {
  if (process.env.PG) return process.env.PG
  const nomes = execFileSync('docker', ['ps', '--format', '{{.Names}}'], {
    stdio: ['ignore', 'pipe', 'pipe'],
  })
    .toString()
    .split('\n')
    .map((s) => s.trim())
    .filter((n) => /postgres/.test(n))
  if (nomes.length === 0) {
    throw new Error('nenhum contentor de Postgres a correr — falta `make dev-up`?')
  }
  if (nomes.length > 1) {
    console.log(`  · vários Postgres a correr, escolhido: ${nomes[0]} (força com PG=)`)
  }
  return nomes[0]
}

/** `psql -tAc` no contentor descoberto, devolvendo a primeira linha. */
export function sql(q, { db = 'delonix_meet', user = 'delonix' } = {}) {
  return execFileSync(
    'docker',
    ['exec', contentorPostgres(), 'psql', '-U', user, '-d', db, '-tAc', q],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  )
    .toString()
    .trim()
    .split('\n')[0]
    .trim()
}
