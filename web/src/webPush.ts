/**
 * Web Push no browser (A-03, ADR-0032): subscrever este browser para receber o
 * toque de uma reunião com a aplicação fechada, e retirá-lo.
 *
 * A decisão de SE se pode oferecer está em [`suportePush`], pura, para se testar
 * sem DOM — e porque é onde mora a condição que mais facilmente falha calada:
 * no iPhone e no iPad, o Web Push só existe com a aplicação INSTALADA no ecrã
 * principal (Safari 16.4+). Fora disso o ecrã tem de o dizer, não oferecer um
 * botão que nunca funciona.
 */
import { pushStatus, subscribePush, unsubscribePush } from './api'
import { chaveEmBytes } from './webPushSuporte'

export { ambienteDoBrowser, suportePush } from './webPushSuporte'
export type { AmbienteDoBrowser, SuportePush } from './webPushSuporte'

/** O registo do service worker, ou `null` — no servidor de desenvolvimento não
 *  há, e esperar pelo `ready` ficava pendurado para sempre. */
async function registo(): Promise<ServiceWorkerRegistration | null> {
  if (!('serviceWorker' in navigator)) return null
  return (await navigator.serviceWorker.getRegistration()) ?? null
}

/** Este browser está subscrito? */
export async function esteBrowserSubscrito(): Promise<boolean> {
  const reg = await registo()
  return !!(await reg?.pushManager.getSubscription())
}

export class PushIndisponivel extends Error {}
export class PushRecusado extends Error {}

/** Pede a permissão, subscreve e regista no servidor. */
export async function subscreverEsteBrowser(): Promise<void> {
  const estado = await pushStatus()
  if (!estado.enabled || !estado.public_key) throw new PushIndisponivel('push desligado nesta instalação')
  const reg = await registo()
  if (!reg) throw new PushIndisponivel('sem service worker')
  const perm = await Notification.requestPermission()
  if (perm !== 'granted') throw new PushRecusado(perm)
  const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: chaveEmBytes(estado.public_key) })
  try {
    await subscribePush(sub.toJSON())
  } catch (e) {
    // O servidor recusou: não se deixa uma subscrição local que nunca recebe nada.
    await sub.unsubscribe().catch(() => false)
    throw e
  }
}

/** Retira este browser: no servidor E no browser. */
export async function retirarEsteBrowser(): Promise<void> {
  const reg = await registo()
  const sub = await reg?.pushManager.getSubscription()
  if (!sub) return
  await unsubscribePush(sub.endpoint)
  await sub.unsubscribe()
}
