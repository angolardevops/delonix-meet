/**
 * As três preferências das legendas, num sítio só.
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
const CHAVE_WEBSPEECH = 'dx_asr_webspeech'

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
 * É seguro como omissão porque degrada para outro motor SOBERANO: sem a rota
 * `/asr` (um servidor que não instalou o Whisper), ou com o serviço em baixo, o
 * motor cai para o Whisper WASM local — não para a Google. Desligar isto NÃO é
 * pedir a Web Speech; é só dispensar o servidor (ver `webSpeechAceita`).
 */
export function asrDoServidorPreferido(): boolean {
  return ler(CHAVE_ASR) !== '0'
}

export function guardaAsrDoServidor(preferido: boolean) {
  gravar(CHAVE_ASR, preferido ? '1' : '0')
}

/**
 * A pessoa ACEITOU, explicitamente, o motor que envia o áudio à Google.
 *
 * `=== '1'` e não `!== '0'`, ao contrário das outras duas preferências deste
 * ficheiro: o que se guarda aqui é o CONSENTIMENTO, e um consentimento que
 * vale por omissão não é consentimento. Um browser que nunca viu o ecrã, ou
 * onde o `localStorage` atira, NÃO aceitou nada.
 *
 * Isto nasceu a 2026-10-18, de uma divergência medida entre a ordem escrita e
 * o código: o ficheiro documentava «1. servidor · 2. wasm · 3. webspeech», e a
 * implementação só alcançava o `wasm` quando o browser NÃO tinha Web Speech
 * (Firefox, Safari) ou com o modo local pedido à mão. Em Chrome, a degradação
 * que mais acontece — o `/asr` a cair a meio da reunião — recuava para a Web
 * Speech, ou seja, começava a enviar o áudio para fora NO MEIO de uma sessão
 * que começara soberana, com um aviso de interface («a usar o motor local»,
 * `media.ts`) que dizia o contrário do que o código fazia.
 */
export function webSpeechAceita(): boolean {
  return ler(CHAVE_WEBSPEECH) === '1'
}

/** Guarda a aceitação (ou a retirada) do motor da Google. */
export function guardaWebSpeech(aceita: boolean) {
  gravar(CHAVE_WEBSPEECH, aceita ? '1' : '0')
}

/**
 * Os motores de transcrição. `nenhum` não é um motor: é o que sobra quando
 * nenhum está alcançável, e existe para que isso se possa DIZER em vez de
 * cair em silêncio no único que envia áudio para fora.
 */
export type MotorDeTranscricao = 'servidor' | 'webspeech' | 'wasm' | 'nenhum'

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
 *  2. **webspeech** — o do Chrome/Edge, e só com TRÊS sins: o browser tem-no, a
 *     organização permite-o (`webSpeechPermitida`, que vem do SERVIDOR) e a
 *     pessoa aceitou-o (`aceitaWebSpeech`). Está aqui, e não no fim, porque uma
 *     escolha explícita que nunca se cumpre não é uma escolha — quem o aceitou
 *     sabendo o que faz recebe-o. Sem os três sins não é alcançável;
 *  3. **wasm** — o recuo por omissão, e o recuo de uma FALHA do servidor:
 *     também não envia áudio a lado nenhum;
 *  4. **nenhum** — o WASM não está servido por esta instalação e a Web Speech
 *     não tem os três sins. Dizer isto é a única saída honesta: a alternativa
 *     é mandar o áudio à Google sem ninguém ter pedido.
 *
 * `temWasm` e `webSpeechPermitida` são omissíveis e valem o SIM — mas por
 * razões opostas, e não as troques:
 *  - `temWasm` porque o modelo é servido pela própria instalação
 *    (`deploy/fetch-whisper.sh`, `web/public/models`): presume-se presente, e
 *    quem o mede (a consola de IA) passa o valor medido. Se a presunção
 *    estiver errada, o `WhisperEngine` falha a arrancar e o chamador diz que
 *    não há motor — a presunção custa uma mensagem, nunca uma fuga;
 *  - `webSpeechPermitida` porque a política chega por WebSocket DEPOIS de a
 *    página abrir, e um `undefined` significa «o servidor ainda não falou».
 *    Não é um problema de soberania: sem `aceitaWebSpeech` o motor da Google
 *    não é alcançável de qualquer maneira, e é por isso que a omissão aqui
 *    pode valer o sim sem abrir nada.
 */
export function motorDaTranscricao(e: {
  asrDoServidor: boolean
  /** O ASR do servidor já falhou nesta sessão. */
  asrDoServidorEmBaixo?: boolean
  temWebSpeech: boolean
  /** O Whisper WASM está servido por esta instalação. Omisso = presume-se que sim. */
  temWasm?: boolean
  /** O chamador pediu explicitamente o motor 100% local. */
  preferirLocal?: boolean
  /** A pessoa aceitou o motor que envia áudio à Google (`dx_asr_webspeech`). */
  aceitaWebSpeech?: boolean
  /** A organização permite a Web Speech (`organizations.allow_web_speech`). */
  webSpeechPermitida?: boolean
}): MotorDeTranscricao {
  if (e.asrDoServidor && !e.asrDoServidorEmBaixo) return 'servidor'
  const wasm = e.temWasm !== false
  // O modo 100% local é um pedido de quem chama (o estúdio), e não há nada
  // acima dele: nem a aceitação guardada no browser o abre.
  if (e.preferirLocal) return wasm ? 'wasm' : 'nenhum'
  if (e.temWebSpeech && e.aceitaWebSpeech === true && e.webSpeechPermitida !== false) {
    return 'webspeech'
  }
  if (wasm) return 'wasm'
  return 'nenhum'
}
