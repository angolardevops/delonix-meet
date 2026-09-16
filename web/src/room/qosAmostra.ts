/**
 * Amostragem de qualidade da chamada — UMA só, para tudo o que a lê.
 *
 * Antes havia duas: uma a cada 2 s só com o painel de participantes aberto (a
 * única que escrevia o estado que o retrato lê) e outra a cada 5 s para a
 * política de camada e o relatório. Com o painel fechado, o «▲ FRACA» ficava
 * preso na última leitura — ou nunca aparecia. Agora a amostra é sempre a
 * mesma chamada a `getStats` que já corria; o painel só a torna mais frequente.
 */

/** Perda (%) acima da qual o retrato diz «ligação fraca». */
export const LIMIAR_PERDA_FRACA = 5

/** 5 s com o painel fechado (a política de camada reage em segundos; R37); 2 s com ele aberto. */
export function intervaloQos(painelAberto: boolean): number {
  return painelAberto ? 2_000 : 5_000
}

/** O relatório ao servidor continua a ~30 s, independentemente do intervalo. */
export const INTERVALO_RELATORIO_MS = 30_000

export function ligacaoFraca(perdaPct: number | undefined): boolean {
  return (perdaPct ?? 0) > LIMIAR_PERDA_FRACA
}

/** Chave estável do conjunto de participantes com ligação fraca. */
export function chaveFracos(byPeer: Record<string, { lossPct: number }>): string {
  return Object.entries(byPeer)
    .filter(([, q]) => ligacaoFraca(q.lossPct))
    .map(([id]) => id)
    .sort()
    .join(',')
}

/**
 * Com o painel fechado, só vale a pena renderizar a sala quando o CONJUNTO de
 * fracos muda (é a única coisa que o palco mostra). Com o painel aberto, os
 * números mudam a cada amostra e mostram-se.
 */
export function deveActualizarQos(painelAberto: boolean, chaveAnterior: string | null, chaveNova: string): boolean {
  return painelAberto || chaveAnterior !== chaveNova
}
