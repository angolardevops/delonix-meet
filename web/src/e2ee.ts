/**
 * E2EE de media com Insertable Streams: cada frame codificado (VP8/opus) é
 * encriptado com AES-GCM ANTES da packetização RTP, por isso nem o SFU nem
 * qualquer intermediário consegue ver/ouvir o conteúdo — só quem tem a chave.
 *
 * A chave é derivada de uma frase-chave partilhada fora da plataforma
 * (PBKDF2-SHA256) e NUNCA é enviada ao servidor.
 *
 * Formato de cada frame: [header em claro | ciphertext+tag | IV(12)]
 * O header (10B keyframe / 3B delta / 1B áudio) fica em claro para os
 * packetizers/depacketizers continuarem a funcionar; vai autenticado como
 * additionalData, por isso não é falsificável.
 */

const WORKER_SRC = `
let key = null
const IV_LEN = 12

function cryptoOffset(kind, type) {
  if (kind === 'video') return type === 'key' ? 10 : 3
  return 1
}

async function encryptFrame(frame, controller, kind) {
  if (!key) { controller.enqueue(frame); return }
  const data = new Uint8Array(frame.data)
  const offset = cryptoOffset(kind, frame.type)
  if (data.byteLength <= offset) { controller.enqueue(frame); return }
  const header = data.slice(0, offset)
  const payload = data.slice(offset)
  const iv = crypto.getRandomValues(new Uint8Array(IV_LEN))
  const ct = new Uint8Array(
    await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: header }, key, payload),
  )
  const out = new Uint8Array(header.byteLength + ct.byteLength + IV_LEN)
  out.set(header, 0)
  out.set(ct, header.byteLength)
  out.set(iv, header.byteLength + ct.byteLength)
  frame.data = out.buffer
  controller.enqueue(frame)
}

async function decryptFrame(frame, controller, kind) {
  if (!key) { controller.enqueue(frame); return }
  const data = new Uint8Array(frame.data)
  const offset = cryptoOffset(kind, frame.type)
  // header + tag GCM (16) + IV — abaixo disto não é um frame nosso.
  if (data.byteLength <= offset + IV_LEN + 16) return
  const header = data.slice(0, offset)
  const iv = data.slice(data.byteLength - IV_LEN)
  const ct = data.slice(offset, data.byteLength - IV_LEN)
  try {
    const pt = new Uint8Array(
      await crypto.subtle.decrypt({ name: 'AES-GCM', iv, additionalData: header }, key, ct),
    )
    const out = new Uint8Array(header.byteLength + pt.byteLength)
    out.set(header, 0)
    out.set(pt, header.byteLength)
    frame.data = out.buffer
    controller.enqueue(frame)
  } catch {
    // Frame que não autentica (chave errada/adulterado): descartar.
  }
}

function pipe(readable, writable, fn, kind) {
  readable
    .pipeThrough(new TransformStream({ transform: (f, c) => fn(f, c, kind) }))
    .pipeTo(writable)
    .catch(() => {})
}

onmessage = async (e) => {
  const m = e.data
  if (m.op === 'key') {
    key = await crypto.subtle.importKey('raw', m.raw, 'AES-GCM', false, ['encrypt', 'decrypt'])
    postMessage({ op: 'key-ready' })
  } else if (m.op === 'encrypt' || m.op === 'decrypt') {
    pipe(m.readable, m.writable, m.op === 'encrypt' ? encryptFrame : decryptFrame, m.kind)
  }
}

// Safari / Firefox: RTCRtpScriptTransform entrega os streams aqui.
onrtctransform = (e) => {
  const t = e.transformer
  pipe(t.readable, t.writable, t.options.op === 'encrypt' ? encryptFrame : decryptFrame, t.options.kind)
}
`

/** O browser suporta encriptação de frames? (Chrome/Edge, Safari, Firefox) */
export function e2eeSupported(): boolean {
  return (
    typeof (window as unknown as { RTCRtpScriptTransform?: unknown }).RTCRtpScriptTransform === 'function' ||
    typeof (RTCRtpSender.prototype as unknown as { createEncodedStreams?: unknown }).createEncodedStreams ===
      'function'
  )
}

/** Deriva a chave AES-256 da frase-chave (nunca sai deste dispositivo). */
export async function deriveRoomKey(passphrase: string, roomCode: string): Promise<ArrayBuffer> {
  const enc = new TextEncoder()
  const material = await crypto.subtle.importKey('raw', enc.encode(passphrase), 'PBKDF2', false, [
    'deriveBits',
  ])
  return crypto.subtle.deriveBits(
    {
      name: 'PBKDF2',
      hash: 'SHA-256',
      salt: enc.encode(`delonix-meet:e2ee:${roomCode}`),
      iterations: 250_000,
    },
    material,
    256,
  )
}

interface LegacySender {
  createEncodedStreams?: () => { readable: ReadableStream; writable: WritableStream }
}
interface LegacyReceiver {
  createEncodedStreams?: () => { readable: ReadableStream; writable: WritableStream }
}
type ScriptTransformCtor = new (worker: Worker, options: { op: string; kind: string }) => unknown

/** Liga o worker de cifra aos senders/receivers de uma RTCPeerConnection. */
export class FrameCrypto {
  private worker: Worker

  constructor() {
    const blob = new Blob([WORKER_SRC], { type: 'application/javascript' })
    this.worker = new Worker(URL.createObjectURL(blob))
  }

  setKey(raw: ArrayBuffer): Promise<void> {
    return new Promise((resolve) => {
      const done = (e: MessageEvent) => {
        if (e.data?.op === 'key-ready') {
          this.worker.removeEventListener('message', done)
          resolve()
        }
      }
      this.worker.addEventListener('message', done)
      this.worker.postMessage({ op: 'key', raw }, [raw])
    })
  }

  protectSender(sender: RTCRtpSender) {
    const kind = sender.track?.kind ?? 'video'
    const ScriptTransform = (window as unknown as { RTCRtpScriptTransform?: ScriptTransformCtor })
      .RTCRtpScriptTransform
    if (ScriptTransform && 'transform' in sender) {
      ;(sender as unknown as { transform: unknown }).transform = new ScriptTransform(this.worker, {
        op: 'encrypt',
        kind,
      })
      return
    }
    const legacy = sender as unknown as LegacySender
    if (legacy.createEncodedStreams) {
      const { readable, writable } = legacy.createEncodedStreams()
      this.worker.postMessage({ op: 'encrypt', kind, readable, writable }, [
        readable as unknown as Transferable,
        writable as unknown as Transferable,
      ])
    }
  }

  protectReceiver(receiver: RTCRtpReceiver, kind: string) {
    const ScriptTransform = (window as unknown as { RTCRtpScriptTransform?: ScriptTransformCtor })
      .RTCRtpScriptTransform
    if (ScriptTransform && 'transform' in receiver) {
      ;(receiver as unknown as { transform: unknown }).transform = new ScriptTransform(this.worker, {
        op: 'decrypt',
        kind,
      })
      return
    }
    const legacy = receiver as unknown as LegacyReceiver
    if (legacy.createEncodedStreams) {
      const { readable, writable } = legacy.createEncodedStreams()
      this.worker.postMessage({ op: 'decrypt', kind, readable, writable }, [
        readable as unknown as Transferable,
        writable as unknown as Transferable,
      ])
    }
  }

  close() {
    this.worker.terminate()
  }
}
