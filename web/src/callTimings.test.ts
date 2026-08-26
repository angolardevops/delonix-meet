import { describe, it, expect } from 'vitest'
import { LinhaDoTempo } from './callTimings'

// A auditoria mediu ZERO métricas de tempo. São as que o cliente pergunta
// primeiro — «quanto demora a entrar?» não se responde com bitrate.

/** Relógio controlado: sem isto, os testes mediriam a velocidade da máquina. */
function relogio() {
  let t = 0
  return { agora: () => t, avancar: (ms: number) => { t += ms } }
}

describe('LinhaDoTempo', () => {
  it('mede o tempo que o UTILIZADOR sente — da intenção até haver media', () => {
    const r = relogio()
    const l = new LinhaDoTempo(r.agora)
    l.marcar('intencao')
    r.avancar(120); l.marcar('token')
    r.avancar(80); l.marcar('ws')
    r.avancar(300); l.marcar('ligado')
    const t = l.resumo()
    expect(t.join_ms).toBe(500)
    // O ws_ms isola a API + rede de sinal do resto: sem ele, um join lento não
    // diz se o problema é o servidor a responder ou o ICE a negociar.
    expect(t.ws_ms).toBe(200)
  })

  it('o «tempo até entrar» conta até haver MEDIA, não até o socket abrir', () => {
    const r = relogio()
    const l = new LinhaDoTempo(r.agora)
    l.marcar('intencao')
    r.avancar(100); l.marcar('ws')
    // Um WebSocket aberto com o ecrã preto não é ter entrado numa reunião.
    expect(l.resumo().join_ms).toBeNull()
    r.avancar(900); l.marcar('ligado')
    expect(l.resumo().join_ms).toBe(1000)
  })

  it('o PRIMEIRO é o primeiro: uma renegociação não reescreve o instante', () => {
    const r = relogio()
    const l = new LinhaDoTempo(r.agora)
    l.marcar('intencao')
    r.avancar(400); l.marcar('primeiro_audio')
    // Meia hora depois, uma renegociação traz áudio "novo".
    r.avancar(1_800_000); l.marcar('primeiro_audio')
    // Sem esta regra, o «tempo até ouvir» passava a medir a última
    // renegociação — um número que parece bom e não quer dizer nada.
    expect(l.resumo().first_audio_ms).toBe(400)
  })

  it('marcos em falta dão null, não zero', () => {
    const l = new LinhaDoTempo(relogio().agora)
    l.marcar('intencao')
    const t = l.resumo()
    // Zero seria uma medição («foi instantâneo»); null é a ausência dela.
    expect(t.join_ms).toBeNull()
    expect(t.first_video_ms).toBeNull()
    expect(t.ice_gathering_ms).toBeNull()
  })

  it('marcos fora de ordem dão null em vez de um número negativo', () => {
    const r = relogio()
    const l = new LinhaDoTempo(r.agora)
    r.avancar(500); l.marcar('intencao')
    // Relógio a andar PARA TRÁS (erro de instrumentação, ou um `performance.now`
    // a ser trocado a meio): o `ligado` fica com um carimbo ANTERIOR ao da
    // intenção. Marcar por ordem errada não chega — os carimbos é que têm de
    // ficar invertidos, e foi isso que a primeira versão deste teste não fez.
    r.avancar(-400)
    l.marcar('ligado')
    // Um -400 num painel é pior do que um buraco: parece um dado.
    expect(l.resumo().join_ms).toBeNull()
  })

  it('conta reinícios de ICE e recuperações', () => {
    const l = new LinhaDoTempo(relogio().agora)
    l.contarReinicioIce(); l.contarReinicioIce(); l.contarRecuperacao()
    const t = l.resumo()
    expect(t.ice_restarts).toBe(2)
    expect(t.reconnects).toBe(1)
  })

  it('a recolha de ICE mede-se da oferta ao fim da recolha', () => {
    const r = relogio()
    const l = new LinhaDoTempo(r.agora)
    l.marcar('intencao')
    r.avancar(200); l.marcar('oferta')
    r.avancar(1500); l.marcar('ice_completo')
    expect(l.resumo().ice_gathering_ms).toBe(1500)
  })

  it('não se reporta uma sessão que nunca chegou a ligar', () => {
    const r = relogio()
    const l = new LinhaDoTempo(r.agora)
    l.marcar('intencao'); r.avancar(9000); l.marcar('ws')
    // Enviesaria a média de «tempo até entrar» com sessões que não entraram.
    // Essas contam-se na taxa de sucesso, não aqui.
    expect(l.vale_reportar()).toBe(false)
    l.marcar('ligado')
    expect(l.vale_reportar()).toBe(true)
  })

  it('`tem` diz se um marco já aconteceu', () => {
    const l = new LinhaDoTempo(relogio().agora)
    expect(l.tem('ligado')).toBe(false)
    l.marcar('ligado')
    expect(l.tem('ligado')).toBe(true)
  })
})
