/**
 * A parte PURA do Web Push no browser (A-03, ADR-0032): o que o ecrã pode
 * oferecer, e a conversão da chave. Vive à parte do `webPush.ts` para se testar
 * sem DOM — o `api.ts`, que esse importa, lê o `localStorage` ao carregar.
 *
 * A condição que mais facilmente falha calada mora aqui: no iPhone e no iPad, o
 * Web Push só existe com a aplicação INSTALADA no ecrã principal (Safari 16.4+).
 */
export type SuportePush =
  /** Tudo o que é preciso existe: pode-se pedir a permissão. */
  | 'pronto'
  /** iPhone/iPad sem a aplicação instalada no ecrã principal. */
  | 'ios-instalar'
  /** Este browser não tem Web Push, ou a aplicação corre sem service worker
   *  (o servidor de desenvolvimento nunca o regista). */
  | 'sem-suporte'
  /** A pessoa recusou as notificações a este site: só ela o desfaz, nas
   *  definições do browser. */
  | 'bloqueado'

export interface AmbienteDoBrowser {
  temServiceWorker: boolean
  temPushManager: boolean
  temNotification: boolean
  permissao: NotificationPermission | null
  ios: boolean
  /** A aplicação corre instalada (`display-mode: standalone`). */
  instalada: boolean
}

export function suportePush(a: AmbienteDoBrowser): SuportePush {
  // Primeiro o iOS: num iPhone sem instalar, o `PushManager` nem existe — e a
  // resposta certa não é «o browser não suporta», é «instale primeiro».
  if (a.ios && !a.instalada) return 'ios-instalar'
  if (!a.temServiceWorker || !a.temPushManager || !a.temNotification) return 'sem-suporte'
  if (a.permissao === 'denied') return 'bloqueado'
  return 'pronto'
}

/** O ambiente real deste browser. */
export function ambienteDoBrowser(): AmbienteDoBrowser {
  const nav = typeof navigator === 'undefined' ? null : navigator
  const ua = nav?.userAgent ?? ''
  // O iPadOS identifica-se como Mac; o que o denuncia são os pontos de toque.
  const ios = /iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && (nav?.maxTouchPoints ?? 0) > 1)
  const instalada =
    typeof window !== 'undefined' &&
    (window.matchMedia?.('(display-mode: standalone)').matches === true ||
      (nav as { standalone?: boolean } | null)?.standalone === true)
  return {
    temServiceWorker: !!nav && 'serviceWorker' in nav,
    temPushManager: typeof window !== 'undefined' && 'PushManager' in window,
    temNotification: typeof window !== 'undefined' && 'Notification' in window,
    permissao: typeof Notification === 'undefined' ? null : Notification.permission,
    ios,
    instalada,
  }
}

/** A `applicationServerKey` em bytes, a partir do base64url que o servidor dá. */
export function chaveEmBytes(b64url: string): Uint8Array<ArrayBuffer> {
  const b64 = b64url.replace(/-/g, '+').replace(/_/g, '/')
  const cheio = b64 + '='.repeat((4 - (b64.length % 4)) % 4)
  const bin = atob(cheio)
  const out = new Uint8Array(new ArrayBuffer(bin.length))
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}
