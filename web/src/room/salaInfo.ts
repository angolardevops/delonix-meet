/**
 * A reunião agendada que interessa a quem está a entrar AGORA: a desta sala
 * cuja hora está mais perto do momento presente.
 */
export function reuniaoMaisProxima<T extends { room_code: string | null; starts_at: string }>(
  meetings: T[],
  code: string,
  agora: number,
): T | null {
  let melhor: T | null = null
  let dist = Infinity
  for (const m of meetings) {
    if (m.room_code !== code) continue
    const d = Math.abs(new Date(m.starts_at).getTime() - agora)
    if (Number.isFinite(d) && d < dist) {
      melhor = m
      dist = d
    }
  }
  return melhor
}
