// Ciclo de vida da chamada SFU/Mesh — a máquina de estados que estava embebida
// no `useEffect` gigante do Room.tsx (avaliação de arquitetura #4, Martin Fowler).
// Extrair a GUARDA para aqui torna as regressões R1/R2 impossíveis por construção
// e testáveis (ver sfuLifecycle.test.ts) — a lógica de arranque deixa de viver
// no meio de 3256 linhas de acoplamento temporal.
//
// R2: o `start` é idempotente (não duplica a chamada) E não monta enquanto o
//     convidado aguarda admissão (`isCancelled`). Montar cedo demais gera oferta
//     stale → glare/rollback → flood → o rate-limit derruba → reload após admitir.
// R1: garantido pelo próprio `create` (o `SfuCall` envia a oferta no construtor);
//     este helper só controla QUANDO se cria, uma única vez, no momento certo.

export interface CallHolderOpts<C> {
  /** Ref partilhada onde a chamada viva é guardada (ex.: `callRef`). */
  ref: { current: C | null }
  /** True quando o efeito foi limpo / o convidado ainda aguarda admissão. */
  isCancelled: () => boolean
  /** Cria a chamada (SfuCall/MeshCall). Só é invocado uma vez, no momento certo. */
  create: () => C
}

/**
 * Devolve a função `start` guardada para o `callHolder`. Preenche-se o holder com
 * isto DEPOIS de media/crypto existirem; o handler `joined` invoca-a após a
 * admissão real. Chamar `start()` mais do que uma vez, ou depois de cancelar, é
 * um no-op seguro.
 */
export function makeCallHolderStart<C>(opts: CallHolderOpts<C>): () => void {
  return () => {
    if (opts.ref.current || opts.isCancelled()) return
    opts.ref.current = opts.create()
  }
}
