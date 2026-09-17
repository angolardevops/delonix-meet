/**
 * Código de sala a partir do que a pessoa colou: o código solto
 * («abc-defg-hij»), um link completo (`…/#/r/abc-defg-hij`) ou o código com
 * espaços e maiúsculas. Devolve `null` quando não há nada que pareça um código.
 */
export function parseRoomCode(raw: string): string | null {
  const s = raw.trim().toLowerCase()
  if (!s) return null
  const fromLink = s.match(/#\/(?:r|lobby)\/([a-z]+(?:-[a-z]+)+)/)
  if (fromLink) return fromLink[1]
  const bare = s.match(/[a-z]+(?:-[a-z]+){2,}/)
  if (bare) return bare[0]
  return null
}
