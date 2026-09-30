/**
 * Os cinco ecrãs do estúdio de TV, e a leitura do ecrã que vem do endereço.
 *
 * Vive à parte do `useSessaoTv` de propósito: a página do Estúdio precisa da
 * LISTA para validar `#/studio?vista=tv&ecra=…` e para desenhar a barra de
 * ecrãs, e não pode importar o `EstudioTv` — esse entra por `lazy()`, e um
 * import estático dele arrastava a mesa de som e as fontes para o chunk que
 * carrega antes de alguém pedir o estúdio de TV.
 */

export type EcraTv = 'mesa-de-corte' | 'mesa-de-som' | 'iluminacao' | 'fontes' | 'cena'

export const ECRAS_TV: readonly EcraTv[] = ['mesa-de-corte', 'mesa-de-som', 'iluminacao', 'fontes', 'cena']

/** O ecrã com que a vista `tv` abre: é na mesa de corte que se corta. */
export const ECRA_TV_INICIAL: EcraTv = 'mesa-de-corte'

/** `null` para o que não é um ecrã — um endereço escrito à mão não parte a vista. */
export function ecraTvDoValor(v: string | null | undefined): EcraTv | null {
  return ECRAS_TV.includes(v as EcraTv) ? (v as EcraTv) : null
}
