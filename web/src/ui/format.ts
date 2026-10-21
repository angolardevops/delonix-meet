/**
 * Formatação de números para o ecrã — **um só sítio**.
 *
 * PORQUE EXISTE: havia TRÊS formatadores de bytes, com TRÊS respostas para o
 * mesmo ficheiro (medido a 2026-10-08, `docs/desenho-2026-10-08-ui-kit-e-dry.md`):
 *
 *   | onde                            | base | 1 500 000 000 bytes dava |
 *   |---------------------------------|------|--------------------------|
 *   | `studio/exports/predefinicoes`  | 1000 | «1,5 GB»                 |
 *   | `pages/admin/orgShared`         | 1024 | «1 GB»                   |
 *   | `pages/recordings/format`       | 1024 | «1,4 GB»                 |
 *
 * A mesma gravação mostrava 1,5 GB no Estúdio, 1 GB no backoffice e 1,4 GB na
 * biblioteca. Não era dívida de arrumação: era o produto a contradizer-se sobre
 * o tamanho do que cobra ao cliente.
 *
 * A BASE É 1024, E O RÓTULO DIZ A VERDADE («GiB», não «GB»). A razão é a quota:
 * o `usage::enforce_recording_quota` do servidor conta BYTES, e um limite de
 * 10 GiB tem de aparecer como 10 GiB — senão quem enche a quota vê um número
 * que não bate com o que lhe foi vendido. Escrever «GB» sobre uma divisão por
 * 1024 seria a mentira do costume; aqui diz-se o que se faz.
 */

/** As unidades binárias, do byte ao tebibyte. O índice é a potência de 1024. */
const UNIDADES = ['B', 'KiB', 'MiB', 'GiB', 'TiB'] as const

/**
 * Tamanho em bytes, legível, na língua activa.
 *
 * Uma casa decimal a partir de MiB e nenhuma abaixo: «820 KiB» e não
 * «820,0 KiB», que é ruído. Zero é «0 B» e não «—»: um ficheiro vazio é um
 * facto, não a ausência de um.
 */
export function formatBytes(bytes: number, locale: string): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  let v = bytes
  let i = 0
  while (v >= 1024 && i < UNIDADES.length - 1) {
    v /= 1024
    i++
  }
  // Abaixo de MiB os decimais não dizem nada a ninguém.
  const casas = i >= 2 ? 1 : 0
  const n = v.toLocaleString(locale, {
    maximumFractionDigits: casas,
    minimumFractionDigits: 0,
  })
  return `${n} ${UNIDADES[i]}`
}
