/**
 * As duas preferências das legendas, num sítio só.
 *
 * PORQUE EXISTE: a mesma chave de localStorage era lida em três lugares com
 * três omissões possíveis — `media.ts` (o motor), `room/useTranscription.ts`
 * (a sala) e `pages/Intelligence.tsx` (a consola, que mostra ao utilizador
 * qual o motor activo). Mudar a omissão num e esquecer os outros dava o pior
 * dos resultados: a consola a dizer «servidor» e a sala a usar o Chrome.
 *
 * SEM IMPORTAÇÕES, de propósito: a `Intelligence.tsx` evita trazer o
 * `media.ts` para o seu chunk, e isto tem de poder entrar lá sem o arrastar.
 *
 * O `localStorage` ATIRA em navegação privada e com os dados do sítio
 * bloqueados — daí os try/catch. Sem ele, valem as omissões.
 */

const CHAVE_CC = 'dx_cc'
const CHAVE_ASR = 'dx_asr_server'

function ler(chave: string): string | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage.getItem(chave)
  } catch {
    return null
  }
}

function gravar(chave: string, valor: string) {
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(chave, valor)
  } catch {
    /* navegação privada: a escolha vale só nesta sessão */
  }
}

/**
 * As legendas ligam-se SOZINHAS ao entrar numa sala — omissão `true`, por
 * decisão do dono a 2026-10-13 («activar a legenda automática»).
 *
 * `!== '0'` e não `=== '1'`: o que se guarda é a RECUSA explícita de quem
 * desligou o botão, e só ela. Um browser que nunca viu a sala, ou onde o
 * localStorage atira, entra com legendas.
 *
 * O que isto implica, e não é pequeno: o que cada um diz passa a ser
 * transcrito e enviado aos outros participantes da sala sem ninguém carregar
 * em nada (`useTranscription`, evento `transcript`). É também o que faz a ata
 * existir — sem linhas transcritas, o servidor não tem o que resumir.
 */
export function legendasLigadasPorOmissao(): boolean {
  return ler(CHAVE_CC) !== '0'
}

/** Guarda a escolha DO UTILIZADOR no botão. Uma falha de microfone não passa por aqui. */
export function guardaLegendas(ligadas: boolean) {
  gravar(CHAVE_CC, ligadas ? '1' : '0')
}

/**
 * O ASR do servidor (`/asr`, `whisper-server/`) é preferido ao do browser —
 * omissão `true`, e a razão é SOBERANIA: a Web Speech do Chrome envia o áudio
 * para servidores da Google.
 *
 * É seguro como omissão porque degrada: sem a rota `/asr` (um servidor que não
 * instalou o Whisper), o motor cai para a Web Speech e, sem ela, para o
 * Whisper WASM local.
 */
export function asrDoServidorPreferido(): boolean {
  return ler(CHAVE_ASR) !== '0'
}

export function guardaAsrDoServidor(preferido: boolean) {
  gravar(CHAVE_ASR, preferido ? '1' : '0')
}

/** Os três motores de transcrição, pela ordem em que o `Transcriber` os tenta. */
export type MotorDeTranscricao = 'servidor' | 'webspeech' | 'wasm'

/**
 * Qual o motor que transcreve, NESTE browser e neste instante.
 *
 * PORQUE É UMA FUNÇÃO E NÃO TRÊS CONDIÇÕES: esta ordem estava escrita duas
 * vezes — no `Transcriber.start` (quem decide) e na consola de IA (quem
 * mostra ao utilizador qual é) — com um comentário a admitir a cópia («a
 * mesma verificação de media.ts, sem trazer media.ts para este chunk»). Com
 * duas cópias, a consola podia dizer «Chrome» enquanto a sala usava o
 * servidor, e nenhuma das duas estaria obviamente errada.
 *
 * A ordem, e as razões:
 *  1. **servidor** — soberano (o áudio não sai da instalação) e o mais preciso.
 *     Só se não tiver JÁ falhado nesta sessão: um serviço em baixo tem de dar
 *     lugar ao seguinte, e tentá-lo outra vez era um ciclo de ligações;
 *  2. **wasm** — sem Web Speech no browser (Firefox, Safari) ou com o modo
 *     100% local pedido: também não envia áudio a lado nenhum;
 *  3. **webspeech** — o do Chrome/Edge. Último de propósito: é o único que
 *     ENVIA O ÁUDIO para fora, aos servidores da Google.
 */
export function motorDaTranscricao(e: {
  asrDoServidor: boolean
  /** O ASR do servidor já falhou nesta sessão. */
  asrDoServidorEmBaixo?: boolean
  temWebSpeech: boolean
  /** O chamador pediu explicitamente o motor 100% local. */
  preferirLocal?: boolean
}): MotorDeTranscricao {
  if (e.asrDoServidor && !e.asrDoServidorEmBaixo) return 'servidor'
  if (!e.temWebSpeech || e.preferirLocal) return 'wasm'
  return 'webspeech'
}
