/**
 * Quando é que remarcar é possível, e porque não quando não é.
 *
 * O servidor recusa com razão (`meeting.recurring_unsupported`,
 * `meeting.already_started`, `meeting.starts_in_past`), mas um botão que só
 * falha depois de se clicar é um botão mal feito: o `Meeting` já traz tudo o
 * que decide isto, por isso decide-se antes.
 *
 * E quando NÃO é possível não se esconde o botão em silêncio — devolve-se a
 * razão, para o ecrã a poder dizer. Um botão que desaparece é um mistério.
 */
import type { Meeting } from '../../api'

export type RazaoNaoRemarca = 'nao-e-anfitriao' | 'ja-comecou' | 'recorrente'

export function razaoNaoRemarca(m: Meeting): RazaoNaoRemarca | null {
  if (!m.is_owner) return 'nao-e-anfitriao'
  if (m.started_at) return 'ja-comecou'
  // Pai de uma série ou uma das suas instâncias: remarcar uma série precisa de
  // semântica própria («esta ocorrência» ou «todas») e o servidor recusa-a.
  if (m.recurrence_freq || m.recurrence_parent_id) return 'recorrente'
  return null
}

export const podeRemarcar = (m: Meeting) => razaoNaoRemarca(m) === null

/**
 * A hora escolhida já passou?
 *
 * O servidor recusa `starts_at <= agora`, e o ecrã tem de o dizer ANTES de
 * mandar — senão a pessoa escolhe uma hora, carrega, e leva um erro que podia
 * ter sido um aviso.
 */
export function horaJaPassou(quando: Date, agora = Date.now()): boolean {
  return quando.getTime() <= agora
}
