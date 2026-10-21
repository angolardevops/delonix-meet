/**
 * A sessão de uma FONTE DE ESTÚDIO — um telefone que entrou por código de
 * emparelhamento, e não como pessoa.
 *
 * É o terceiro tipo de quem entra numa sala, a par do membro (tem conta) e do
 * convidado (tem bilhete). Uma fonte não tem conta nem nome de pessoa: tem o
 * token que o resgate lhe deu (`typ: "source"`, doze horas, só esta sala), a
 * etiqueta que o Estúdio lhe pôs — `CAM 1 · Fold6` — e os servidores ICE que
 * vieram no mesmo resgate, porque sem conta não pode pedi-los ao
 * `/api/ice-servers`.
 *
 * Guarda-se no `sessionStorage`, pela sala, como o bilhete de convidado:
 * sobrevive ao F5 com que a sala reentra depois de uma quebra, e morre com o
 * separador. Perder isto não perde a câmara — perde-se a identidade de FONTE,
 * e o telefone voltaria a entrar como um convidado qualquer, que foi
 * exactamente o defeito que este módulo fecha.
 */
import type { Room } from './api'

export interface SessaoDeFonte {
  code: string
  roomToken: string
  iceServers: RTCConfiguration
  sourceId: string
  studioId: string
  /** CAM n. */
  number: number
  /** `CAM 1 · Fold6`. */
  label: string
  /** Epoch ms em que o token deixa de servir. */
  expiraEm: number
}

const chave = (code: string) => `dx_fonte_${code}`

function sessao(): Storage | null {
  try {
    return globalThis.sessionStorage ?? null
  } catch {
    return null // modo privado, ou armazenamento bloqueado
  }
}

export function guardarFonte(s: SessaoDeFonte): SessaoDeFonte {
  try {
    sessao()?.setItem(chave(s.code), JSON.stringify(s))
  } catch {
    /* sem armazenamento: vale enquanto a página viver */
  }
  return s
}

export function fonteDe(code: string, agora = Date.now()): SessaoDeFonte | null {
  try {
    const cru = sessao()?.getItem(chave(code))
    if (!cru) return null
    const s = JSON.parse(cru) as SessaoDeFonte
    // Um token expirado não se renova sozinho: uma fonte não tem credencial
    // para pedir outro. Esquece-se, e quem quiser voltar pede um código novo
    // ao operador — que é o desenho, não uma limitação por resolver.
    if (!s.roomToken || !(s.expiraEm > agora)) {
      esquecerFonte(code)
      return null
    }
    return s
  } catch {
    return null
  }
}

export function esquecerFonte(code: string): void {
  try {
    sessao()?.removeItem(chave(code))
  } catch {
    /* nada a esquecer */
  }
}

/**
 * A sala como a fonte a vê. Vem toda do token: o `code` do resgate e a
 * topologia que o servidor já lá pôs. Uma fonte nunca é dona da sala e nunca
 * passa pela sala de espera — o código de emparelhamento JÁ foi a admissão,
 * dado por quem tem o Estúdio aberto.
 */
export function salaDaFonte(s: SessaoDeFonte, topology = 'sfu'): Room {
  return {
    id: '',
    code: s.code,
    name: s.label,
    owner_id: '',
    topology,
    waiting_room: false,
    e2ee: false,
    format: 'meeting',
  }
}
