// A QUE vídeo se aplicam o realce e o palco imersivo, e onde o encontrar.
//
// A decisão é pura (testada). A procura no DOM é o único ponto que conhece a
// marcação do palco: os atributos `data-peer-id` do retrato e
// `data-presentation` da apresentação — identidade estável, que não muda
// quando muda a decoração (R90). Se a marcação mudar, é aqui e só aqui.

export type EnhanceTarget = { kind: 'presentation'; peerId: string } | { kind: 'peer'; peerId: string; name: string }

export interface TargetInput {
  /** Quem apresenta; `'me'` quando sou eu. */
  presentationPeerId: string | null
  viewMode: 'grid' | 'stage'
  stagePeerId: string | null
  stageOnSelf: boolean
  peers: { peerId: string; username: string; camOn: boolean }[]
}

/**
 * Realce: o que está em DESTAQUE e vem de outra pessoa. A apresentação remota
 * primeiro (é texto — onde o realce mais se nota), depois o orador em palco.
 * Nunca a minha própria imagem, nem a minha apresentação: o realce existe para
 * compensar o que a rede fez ao vídeo que RECEBO.
 */
export function sharpenTarget(i: TargetInput): EnhanceTarget | null {
  if (i.presentationPeerId && i.presentationPeerId !== 'me') return { kind: 'presentation', peerId: i.presentationPeerId }
  if (i.presentationPeerId) return null
  return stagePeer(i)
}

/**
 * Palco imersivo: só o orador em destaque, com câmara, fora de apresentações.
 * Numa grelha não há «orador em destaque» a quem dar profundidade.
 */
export function immersiveTarget(i: TargetInput): EnhanceTarget | null {
  if (i.presentationPeerId) return null
  return stagePeer(i)
}

function stagePeer(i: TargetInput): EnhanceTarget | null {
  if (i.viewMode !== 'stage' || i.stageOnSelf || !i.stagePeerId) return null
  const p = i.peers.find((x) => x.peerId === i.stagePeerId)
  if (!p || !p.camOn) return null
  return { kind: 'peer', peerId: p.peerId, name: p.username }
}

export function sameTarget(a: EnhanceTarget | null, b: EnhanceTarget | null): boolean {
  return a?.kind === b?.kind && a?.peerId === b?.peerId
}

/** O `<video>` do alvo no palco, ou `null` se ainda não está montado. */
export function findTargetVideo(target: EnhanceTarget, root: ParentNode = document): HTMLVideoElement | null {
  if (target.kind === 'presentation') {
    return root.querySelector<HTMLVideoElement>('.rm-stage__main [data-presentation="remota"] video')
  }
  const id = typeof CSS !== 'undefined' && CSS.escape ? CSS.escape(target.peerId) : target.peerId.replace(/["\\]/g, '\\$&')
  return root.querySelector<HTMLVideoElement>(`.rm-stage__main [data-peer="remoto"][data-peer-id="${id}"] > video`)
}

/** Guarda pequena: um valor que só notifica quem o lê quando muda. */
export function createStore<T>(initial: T) {
  let value = initial
  const subs = new Set<() => void>()
  return {
    get: () => value,
    set(next: T) {
      if (Object.is(next, value)) return
      value = next
      subs.forEach((f) => f())
    },
    subscribe(f: () => void) {
      subs.add(f)
      return () => {
        subs.delete(f)
      }
    },
  }
}

export type Store<T> = ReturnType<typeof createStore<T>>
