/**
 * Porque é que o `/ws` recusou — perguntado ao servidor, porque o browser não
 * nos diz.
 *
 * Um WebSocket recusado no HANDSHAKE não entrega o estado HTTP nem o corpo:
 * chega um `onclose` com `code: 1006`, indistinguível de uma falha de rede. A
 * pessoa recusada por capacidade via «Ligação instável. Verifica a rede» — com
 * a rede dela perfeita — e a página entrava em ciclo de `location.reload()`.
 *
 * A sondagem vai ao MESMO prefixo `/ws`, com o mesmo `room=`: é o único com
 * afinidade por `$arg_room` (ADR-0001), logo cai no MESMO pod que recusou. Em
 * `/api` cairia num pod ao acaso, com a contagem de outro nó.
 */

/** Os motivos que o servidor sabe nomear (`Admissao::code` em signaling.rs). */
export type MotivoDeRecusa =
  | 'org_at_capacity'
  | 'node_at_capacity'
  | 'over_fair_share'
  /** O nó está a encerrar. NÃO é tecto: repete-se, e cai-se noutro pod. */
  | 'node_draining'

const MOTIVOS: Record<string, MotivoDeRecusa> = {
  'admission.org_at_capacity': 'org_at_capacity',
  'admission.node_at_capacity': 'node_at_capacity',
  'admission.over_fair_share': 'over_fair_share',
  'admission.node_draining': 'node_draining',
}

/**
 * Vale a pena voltar a tentar? Um TECTO não passa por repetir — repetir só
 * amplifica a sobrecarga que a admissão existe para conter. Um drain, sim: é
 * para isso que ele recusa, e o balanceador manda para outro pod.
 *
 * `null` (não sabemos porquê) conta como «repetir»: era o que recuperava de um
 * 502 do ingress a meio de um rollout, ou de um room token expirado, e
 * desistir aí deixava a pessoa num ecrã morto sem nada a não ser F5 à mão.
 */
export function valeRepetir(motivo: MotivoDeRecusa | null): boolean {
  return motivo === null || motivo === 'node_draining'
}

/**
 * O motivo do corpo de uma resposta da sondagem, ou `null` se a resposta não
 * nomeia nenhum que saibamos traduzir.
 *
 * Separada da chamada de rede para ser testável sem servidor — e porque é aqui
 * que mora a decisão de NÃO mostrar o `code` cru a ninguém: o código é
 * contrato, a frase é da interface.
 */
export function motivoDoCorpo(corpo: unknown): MotivoDeRecusa | null {
  if (typeof corpo !== 'object' || corpo === null) return null
  const code = (corpo as { code?: unknown }).code
  return typeof code === 'string' ? (MOTIVOS[code] ?? null) : null
}

/**
 * Pergunta ao servidor porque recusaria o socket. `null` = não sabemos (a
 * sondagem falhou, ou o servidor diz que entraríamos agora — nesse caso a
 * recusa foi mesmo de rede e a frase genérica é a honesta).
 *
 * Nunca lança: isto corre no caminho de uma falha, e falhar a explicar uma
 * falha não pode piorar o que a pessoa vê.
 */
export async function perguntarPorqueRecusou(
  roomToken: string,
  roomCode?: string,
): Promise<MotivoDeRecusa | null> {
  const sala = roomCode ? `&room=${encodeURIComponent(roomCode)}` : ''
  try {
    const r = await fetch(`/ws/admission?token=${encodeURIComponent(roomToken)}${sala}`)
    // 204: entraríamos agora — a recusa não foi de admissão.
    if (r.ok) return null
    return motivoDoCorpo(await r.json().catch(() => null))
  } catch {
    return null
  }
}
