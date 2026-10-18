/**
 * O ESTÚDIO desta organização — a entidade do servidor (`/api/orgs/{org}/studios`),
 * não o ecrã.
 *
 * Existe porque dois sítios precisam do mesmo: o cartão de emparelhamento, para
 * emitir códigos, e a sala do Estúdio, para saber em que sala entrar. Antes
 * disto a sala era CRIADA na hora (`createRoom`) e o telefone emparelhado
 * entrava na sala do registo — duas salas diferentes, e o telefone, embora
 * ligado (`connected = true` no servidor), nunca aparecia no Estúdio.
 *
 * A regra que isto fixa: **a sala do estúdio é a do registo**. É estável entre
 * sessões, é a que o código de emparelhamento promete, e é por isso que o
 * `studios.room_code` existe.
 *
 * Um estúdio por organização chega para já — o produto ainda não tem onde
 * escolher entre vários. Quando tiver, isto passa a receber o id.
 */
import { createStudio, listStudios, myOrgs, type Studio } from '../api'

export interface EstudioActual {
  orgId: string
  estudio: Studio
}

/**
 * Devolve o estúdio da organização, criando-o se ainda não existir.
 *
 * O `listStudios` devolve o ENVELOPE `{items}`, como as outras listagens — lê-lo
 * como um array dava `undefined` e criava um estúdio novo a cada chamada.
 */
export async function estudioActual(nomeSeNovo: string): Promise<EstudioActual | null> {
  const orgs = await myOrgs()
  const org = orgs[0]
  if (!org) return null
  const { items } = await listStudios(org.id)
  const estudio = items[0] ?? (await createStudio(org.id, nomeSeNovo))
  return { orgId: org.id, estudio }
}
