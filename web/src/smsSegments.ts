/**
 * Estimativa de segmentos de um SMS, para o contador do formulário.
 *
 * É uma ESTIMATIVA: quem codifica e segmenta de verdade é o servidor
 * (`server/src/sms_codec.rs`, ADR-0005) e o número final vem na `Message`
 * (`encoding`, `segments`). Isto só existe para a pessoa ver, enquanto escreve,
 * que um «ã» passa a mensagem para UCS-2 e corta a capacidade para menos de
 * metade — e isso custa dinheiro.
 *
 * Regras (3GPP TS 23.038 / GSM 03.38):
 *  · GSM-7: 160 septetos numa parte; 153 por parte se houver mais do que uma
 *    (7 septetos vão para o cabeçalho de concatenação).
 *  · Caracteres da tabela de EXTENSÃO custam 2 septetos (ESC + carácter).
 *  · Qualquer carácter fora da tabela básica + extensão → UCS-2: 70 unidades
 *    numa parte, 67 por parte; conta-se em unidades UTF-16 (um emoji fora do
 *    BMP são 2).
 *
 * Armadilha da tabela: na posição 0x09 está o `Ç` MAIÚSCULO. O `ç` minúsculo
 * NÃO está na tabela básica — e é por isso que «Olá, obrigado pela atenção»
 * cabe em GSM-7 mas «Serviço» não.
 */

/** Tabela básica GSM 03.38 (sem o ESC em 0x1B). */
const GSM7_BASIC =
  '@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ' +
  ' !"#¤%&\'()*+,-./0123456789:;<=>?' +
  '¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§' +
  '¿abcdefghijklmnopqrstuvwxyzäöñüà'

/** Tabela de extensão (cada um custa ESC + carácter = 2 septetos). */
const GSM7_EXT = '\f^{}\\[~]|€'

const BASIC = new Set(Array.from(GSM7_BASIC))
const EXT = new Set(Array.from(GSM7_EXT))

export type SmsEncoding = 'gsm7' | 'ucs2'

export interface SmsEstimate {
  encoding: SmsEncoding
  /** Septetos (GSM-7) ou unidades UTF-16 (UCS-2). */
  units: number
  /** 0 para corpo vazio. */
  segments: number
  /** Capacidade por parte com o número de partes actual (160/153 ou 70/67). */
  perSegment: number
}

export function estimateSms(body: string): SmsEstimate {
  let septets = 0
  let gsm = true
  for (const ch of body) {
    if (BASIC.has(ch)) septets += 1
    else if (EXT.has(ch)) septets += 2
    else {
      gsm = false
      break
    }
  }

  if (gsm) {
    const single = 160
    const multi = 153
    const segments = septets === 0 ? 0 : septets <= single ? 1 : Math.ceil(septets / multi)
    return { encoding: 'gsm7', units: septets, segments, perSegment: segments > 1 ? multi : single }
  }

  // `length` de uma string JS já são unidades UTF-16.
  const units = body.length
  const single = 70
  const multi = 67
  const segments = units <= single ? 1 : Math.ceil(units / multi)
  return { encoding: 'ucs2', units, segments, perSegment: segments > 1 ? multi : single }
}
