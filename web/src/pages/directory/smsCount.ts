/**
 * Contador de SMS da consola — espelho, em TypeScript, da regra ÚNICA do
 * servidor (`server/src/sms_codec.rs`) e do corpo que ele monta
 * (`sms::contact_body`). Não decide nada: o servidor volta a codificar e
 * recusa acima de `SMS_MAX_SEGMENTS`. Serve para quem escreve saber, ANTES de
 * enviar, que um «ç» passa a mensagem a UCS-2 e cada parte a 67 caracteres.
 */

/** `sms_codec::MAX_SEGMENTS`. */
export const SMS_MAX_SEGMENTS = 6

// Tabela base GSM 03.38 (sem o escape 0x1B, que não é um carácter escrevível).
const GSM_BASIC = new Set(
  Array.from(
    '@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !"#¤%&\'()*+,-./0123456789:;<=>?¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà',
  ),
)
// Tabela de extensão: cada um custa DOIS septetos.
const GSM_EXTENSION = new Set(Array.from('\f^{}\\[~]|€'))

export interface SmsCount {
  encoding: 'gsm7' | 'ucs2'
  /** Septetos (GSM-7) ou unidades UTF-16 (UCS-2) do texto inteiro. */
  units: number
  segments: number
}

/** Divide unidades indivisíveis como o `split_units` do servidor. */
function countParts(sizes: number[], single: number, perPart: number): number {
  const total = sizes.reduce((a, b) => a + b, 0)
  if (total <= single) return 1
  let parts = 0
  let current = 0
  for (const s of sizes) {
    if (current + s > perPart) {
      parts += 1
      current = 0
    }
    current += s
  }
  return current > 0 ? parts + 1 : parts
}

export function countSms(text: string): SmsCount {
  if (!text) return { encoding: 'gsm7', units: 0, segments: 0 }
  const chars = Array.from(text)
  const septets: number[] = chars.map((c) => (GSM_BASIC.has(c) ? 1 : GSM_EXTENSION.has(c) ? 2 : 0))
  if (septets.every((n) => n > 0)) {
    return { encoding: 'gsm7', units: septets.reduce((a, b) => a + b, 0), segments: countParts(septets, 160, 153) }
  }
  // UCS-2: um carácter são 1 ou 2 unidades (par substituto), que ficam juntas.
  // O servidor conta em octetos (140/134); aqui em unidades (70/67).
  const units = chars.map((c) => c.length)
  return { encoding: 'ucs2', units: units.reduce((a, b) => a + b, 0), segments: countParts(units, 70, 67) }
}

/** `sms::contact_body`: o nome de quem envia (até 40 caracteres) vai à frente. */
export function contactBody(sender: string, body: string): string {
  const name = Array.from(sender.trim()).slice(0, 40).join('')
  return `${name} (Delonix Meet): ${body.trim()}`
}
