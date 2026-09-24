/**
 * A transcrição que o SERVIDOR já fez de uma gravação da biblioteca, trazida
 * para o projecto do Estúdio.
 *
 * Porque existe: uma gravação aberta da biblioteca pode já ter transcrição com
 * segmentos no servidor (`GET /api/recordings/{id}/transcript`, ai-worker), e o
 * Estúdio só sabia transcrever no browser — com o modelo em falta o botão
 * ficava desactivado e a gravação transcrita aparecia «sem transcrição».
 *
 * Os segmentos vêm em tempo da FONTE (a gravação); o projecto vive em tempo da
 * LINHA. A conversão passa pelos clipes dessa fonte (os trechos já cortados
 * não entram) e o servidor só dá tempos por segmento — as palavras recebem
 * tempos estimados, e a legenda fica marcada como tal.
 */
import type { TranscriptSegment } from '../../api'
import type { Cue, Palavra, Projecto } from '../edit/projecto'
import { clipsDaFaixa, intervalosDaFonteNaLinha } from '../edit/projecto'
import { distribuirPalavras, palavrasParaCues, REGRAS } from './legendas'

/** A fonte do projecto que veio da biblioteca e está na linha (áudio ou vídeo). */
export function fonteDaBiblioteca(p: Projecto): { fonteId: string; gravacao: string } | null {
  for (const f of p.fontes) {
    if (!f.gravacao) continue
    if (p.clips.some((c) => c.fonteId === f.id && (c.faixa === 'A1' || c.faixa === 'V1'))) return { fonteId: f.id, gravacao: f.gravacao }
  }
  return null
}

/** Segmentos do servidor → cues do projecto, em tempo da linha. */
export function cuesDoServidor(p: Projecto, fonteId: string, segmentos: TranscriptSegment[], orador?: string): Cue[] {
  const faixa = clipsDaFaixa(p, 'A1').some((c) => c.fonteId === fonteId) ? 'A1' : 'V1'
  const palavras: Palavra[] = []
  for (const s of [...segmentos].sort((a, b) => a.start_ms - b.start_ms)) {
    const texto = s.text.trim()
    if (!texto) continue
    const inicio = Math.max(0, s.start_ms) / 1000
    const fim = Math.max(s.end_ms, s.start_ms + 200) / 1000
    const pedacos = intervalosDaFonteNaLinha(p, fonteId, faixa, [{ inicio, fim }])
    if (!pedacos.length) continue
    // Um corte a meio do segmento parte-o em pedaços: as palavras distribuem-se
    // pela duração que SOBROU, e cada uma cai no pedaço onde lhe calha.
    const total = pedacos.reduce((n, x) => n + x.fim - x.inicio, 0)
    for (const w of distribuirPalavras(texto, 0, total)) {
      let resto = w.inicio
      for (const x of pedacos) {
        const d = x.fim - x.inicio
        if (resto < d || x === pedacos[pedacos.length - 1]) {
          const a = x.inicio + Math.min(resto, d)
          palavras.push({ texto: w.texto, inicio: a, fim: Math.min(x.fim, a + (w.fim - w.inicio)) })
          break
        }
        resto -= d
      }
    }
  }
  return palavrasParaCues(palavras, REGRAS, orador)
}

/** Cues do projecto → segmentos para os pedidos de IA (tempo da linha, em ms). */
export function segmentosDasCues(cues: Cue[]): TranscriptSegment[] {
  return cues
    .filter((c) => c.texto.trim())
    .map((c) => ({ start_ms: Math.round(c.inicio * 1000), end_ms: Math.round(c.fim * 1000), text: c.texto.trim(), confidence: null }))
}
