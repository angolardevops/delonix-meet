/**
 * O hook que liga o catálogo de atalhos (`ui/atalhos.ts`) a um ecrã: um só
 * `keydown` por escopo, e as acções declaradas por `id`.
 *
 * Porque é por `id` e não pela tecla: a tecla escreve-se UMA vez, no catálogo,
 * que é também o que a folha de atalhos («?») mostra. Um ecrã que quisesse
 * mudar a tecla tinha de a mudar no catálogo — e a folha acompanha sem
 * ninguém se lembrar dela.
 *
 * As acções vivem numa ref: um ecrã pode passar closures novas a cada render
 * (o caso normal) sem voltar a ligar e desligar o ouvinte.
 */
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { atalhoPorId, atalhosDoEscopo, combina, type EscopoDeAtalho, type IdDeAtalho, teclasDoAtalho } from './atalhos'

export type AccoesDeAtalho = Partial<Record<IdDeAtalho, (() => void) | undefined>>

/**
 * Liga os atalhos de `escopo` cujas acções foram dadas. Um `id` sem acção (ou
 * `undefined`, o caso de um controlo indisponível) não consome a tecla.
 *
 * `activo = false` desliga o ouvinte — é o que o Estúdio faz quando a vista
 * de TV está no ar e a mesa de corte quer as teclas para si.
 */
export function useAtalhos(escopo: EscopoDeAtalho, accoes: AccoesDeAtalho, activo = true): void {
  const ref = useRef(accoes)
  ref.current = accoes
  useEffect(() => {
    if (!activo) return
    const entradas = atalhosDoEscopo(escopo)
    const onKey = (e: KeyboardEvent) => {
      for (const a of entradas) {
        const fn = ref.current[a.id]
        if (!fn || !combina(a.combinacao, e)) continue
        e.preventDefault()
        fn()
        return
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [escopo, activo])
}

/**
 * A dica de um controlo que também tem atalho: `Edição · ⌘⇧2`. O separador
 * vive no dicionário, não no código — é a regra das frases visíveis.
 *
 * Um atalho que só existe no teclado é um atalho que ninguém encontra: todo o
 * controlo com tecla devia dizê-lo no `title`.
 */
export function useDicaDeAtalho(): (id: IdDeAtalho, acao: string) => string {
  const { t } = useTranslation()
  return (id, acao) => t('ui.atalhos.dica', { acao, tecla: teclasDoAtalho(atalhoPorId(id)) })
}
