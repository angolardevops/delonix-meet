/**
 * O que fundir quando o servidor e este browser têm o mesmo diagrama
 * (ADR-0020).
 *
 * **Porque é um ficheiro só com funções puras.** A parte difícil da
 * sincronização não é falar com a rede — é decidir quem ganha. Essa decisão
 * mede-se com uma tabela de casos e sem IndexedDB nem `fetch` pelo meio; o
 * `store.ts` fica só com o I/O.
 *
 * **A regra, e o que ela NÃO é.** Ganha o `updatedAt` mais recente, documento
 * inteiro. Não há merge de nós: duas pessoas a editar o mesmo diagrama ao mesmo
 * tempo não é um caso que isto resolva, e prometer o contrário era pior do que
 * não prometer nada. O que isto resolve é o caso real — a MESMA pessoa noutro
 * computador, ou a voltar depois de estar offline.
 *
 * **O empate conta.** Dois `updatedAt` iguais significam o mesmo documento (foi
 * um que veio do outro), e aí não se faz nada: tratar o empate como «o servidor
 * ganha» punha um `GET` e um `PUT` em cada abertura de cada diagrama.
 */

/** O mínimo que as duas pontas têm em comum. */
export interface Sincronizavel {
  id: string
  updatedAt: string
}

export type Lado = 'igual' | 'enviar' | 'descarregar' | 'so-local' | 'so-servidor'

/**
 * Compara um diagrama nas duas pontas. `undefined` de um lado quer dizer «não
 * existe lá».
 *
 * Datas em ISO 8601 UTC comparam-se como TEXTO — é por isso que o `updatedAt`
 * é `toISOString()` e não um número. Com fusos diferentes a comparação textual
 * mentiria; o `toISOString()` é sempre `Z`.
 */
export function compara(local?: Sincronizavel, servidor?: Sincronizavel): Lado {
  if (local && !servidor) return 'so-local'
  if (!local && servidor) return 'so-servidor'
  if (!local || !servidor) return 'igual' // nenhum dos dois: nada a fazer
  const l = Date.parse(local.updatedAt)
  const s = Date.parse(servidor.updatedAt)
  // Uma data que não se lê não decide nada: fica como está, e o próximo
  // `PUT` normal resolve. Apagar ou escrever por cima com base num
  // `NaN` era a maneira de perder trabalho.
  if (Number.isNaN(l) || Number.isNaN(s)) return 'igual'
  // Ao MILISSEGUNDO: o Postgres guarda microssegundos e o `Date` do browser
  // não, pelo que o que volta do servidor é sempre «ligeiramente anterior» ao
  // que lá está. Sem este arredondamento, cada gravação ficava eternamente «por
  // enviar».
  if (Math.floor(l / 1) === Math.floor(s / 1)) return 'igual'
  return l > s ? 'enviar' : 'descarregar'
}

export interface Plano<T extends Sincronizavel> {
  /** O que mostrar na lista agora: o mais recente de cada lado, por data. */
  lista: T[]
  /** Ids a enviar para o servidor (só aqui, ou aqui mais novo). */
  enviar: string[]
  /** Ids a trazer do servidor (só lá, ou lá mais novo). */
  descarregar: string[]
}

/**
 * O plano para uma lista inteira. A `lista` que sai é o que o ecrã mostra
 * imediatamente — já com os que só existem no servidor —, e as duas filas são o
 * trabalho de fundo.
 */
export function planeia<T extends Sincronizavel>(locais: T[], servidor: T[]): Plano<T> {
  const porId = new Map<string, { l?: T; s?: T }>()
  for (const l of locais) porId.set(l.id, { ...(porId.get(l.id) ?? {}), l })
  for (const s of servidor) porId.set(s.id, { ...(porId.get(s.id) ?? {}), s })

  const lista: T[] = []
  const enviar: string[] = []
  const descarregar: string[] = []
  for (const [id, { l, s }] of porId) {
    switch (compara(l, s)) {
      case 'so-local':
        enviar.push(id)
        break
      case 'so-servidor':
        descarregar.push(id)
        break
      case 'enviar':
        enviar.push(id)
        break
      case 'descarregar':
        descarregar.push(id)
        break
    }
    // Mostra-se o mais recente dos dois. Um diagrama que só está no servidor
    // aparece na lista ANTES de ser descarregado: é o que faz «abri noutro
    // computador» parecer imediato.
    const melhor = compara(l, s) === 'descarregar' || !l ? (s ?? l) : l
    if (melhor) lista.push(melhor)
  }
  lista.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
  return { lista, enviar, descarregar }
}
