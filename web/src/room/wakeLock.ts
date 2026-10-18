/**
 * Manter o ecrã aceso enquanto o telefone serve de CÂMARA.
 *
 * PORQUE EXISTE. Um telefone no tripé é uma câmara: ninguém lhe toca durante
 * a emissão. Sem wake lock o ecrã apaga ao fim do tempo de ociosidade do
 * aparelho (30 s a 2 min por omissão) e, com ele, o browser suspende a aba —
 * a CAM cai no meio do directo. A funcionalidade inteira do #356 depende
 * disto, e a única mitigação sem ele é mexer no ecrã de minuto a minuto.
 *
 * A ARMADILHA, e é a razão de isto não ser uma linha. O wake lock de ecrã é
 * libertado pelo PRÓPRIO browser quando a página deixa de estar visível
 * (atender uma chamada, ver uma notificação, trocar de aba) e **NÃO volta
 * sozinho**. Quem peça só na montagem fica sem lock para o resto da sessão,
 * e o sintoma é idêntico a nunca o ter pedido — por isso o `visibilitychange`
 * é parte da correcção, não um extra.
 *
 * A lógica vive aqui, fora do React e com as dependências injectadas, porque
 * é isto que se tem de poder medir: que se pede, que NÃO se pede escondido,
 * que se volta a pedir ao reaparecer, e que não se pede duas vezes com um
 * lock vivo. O hook é a ligação fina (ver `usarWakeLock`).
 */

/** O que a API devolve. Só se usa `released` e `release()`. */
export interface Sentinela {
  released: boolean
  release: () => Promise<void>
}

export interface AmbienteWakeLock {
  /** `navigator.wakeLock`, ou `undefined` onde a API não existe (iOS < 16.4, Firefox). */
  pedirAoSistema?: (tipo: 'screen') => Promise<Sentinela>
  /** `document.visibilityState`. Lido a cada tentativa, nunca guardado. */
  visivel: () => boolean
}

export interface GuardaDeEcra {
  /** Pede o lock se faltar e a página estiver visível. Idempotente. */
  pedir: () => Promise<void>
  /** Larga o que estiver vivo. Idempotente, e seguro depois de `parar`. */
  largar: () => Promise<void>
  /** Depois disto nenhum `pedir` tardio (uma promessa em voo) pega. */
  parar: () => void
  /** Só para os testes e para o ecrã: há lock vivo? */
  activo: () => boolean
  /** Quantas vezes se pediu ao sistema. Mede a idempotência. */
  pedidos: () => number
}

export function criarGuardaDeEcra(amb: AmbienteWakeLock, aoFalhar?: (e: unknown) => void): GuardaDeEcra {
  let sentinela: Sentinela | null = null
  let vivo = true
  let pedidos = 0
  let emVoo = false

  const temLock = () => sentinela !== null && !sentinela.released

  return {
    activo: temLock,
    pedidos: () => pedidos,
    parar() {
      vivo = false
    },
    async pedir() {
      if (!vivo || !amb.pedirAoSistema) return
      // Escondido não se pede: o browser recusa (`NotAllowedError`) e, pior,
      // um erro aqui gastava a mensagem de falha num caso que é normal.
      if (!amb.visivel()) return
      if (temLock() || emVoo) return
      emVoo = true
      pedidos += 1
      try {
        const s = await amb.pedirAoSistema('screen')
        // Entre o `await` e aqui a página pode ter sido desmontada: largar
        // logo, senão o lock sobrevive à página que o pediu.
        if (!vivo) {
          void s.release().catch(() => {})
          return
        }
        sentinela = s
      } catch (e) {
        aoFalhar?.(e)
      } finally {
        emVoo = false
      }
    },
    async largar() {
      const s = sentinela
      sentinela = null
      if (!s || s.released) return
      try {
        await s.release()
      } catch {
        /* a página está a fechar; não há a quem contar */
      }
    },
  }
}

/** O ambiente real do browser. Separado para o teste poder dar outro. */
export function ambienteDoBrowser(): AmbienteWakeLock {
  const nav = navigator as Navigator & {
    wakeLock?: { request: (tipo: 'screen') => Promise<Sentinela> }
  }
  return {
    pedirAoSistema: nav.wakeLock ? (tipo) => nav.wakeLock!.request(tipo) : undefined,
    visivel: () => document.visibilityState === 'visible',
  }
}
