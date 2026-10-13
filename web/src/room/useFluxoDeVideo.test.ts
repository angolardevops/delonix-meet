/**
 * O retrato só diz «sem imagem» quando não há mesmo imagem.
 *
 * O caso que deu origem a isto (produção, 2026-10-13): um participante em macOS
 * publicou a faixa de vídeo e anunciou a câmara ligada, e nunca chegou um
 * quadro. O `<video>` ficava preto e quem via não tinha como distinguir isso de
 * uma avaria nossa.
 *
 * O que estes testes guardam é a PRUDÊNCIA da regra — não acusar cedo de mais,
 * porque uma chamada demora segundos a assentar e um rótulo a piscar a cada
 * reconexão seria pior do que o preto — e a distinção entre «nunca houve
 * imagem» e «havia e parou», que são duas coisas diferentes para quem vê.
 */
import { describe, expect, it } from 'vitest'
import { proximoFluxo, vigiaNovo, type Vigia } from './useFluxoDeVideo'

const RELOGIO = { primeiro: 4000, parado: 6000 }
const T0 = 1_000_000

describe('proximoFluxo', () => {
  it('sem um único quadro, antes do prazo: ainda «a chegar»', () => {
    const v = vigiaNovo(T0)
    const r = proximoFluxo({ videoWidth: 0, currentTime: 0 }, v, T0 + 3000, RELOGIO)
    expect(r.fluxo).toBe('a-chegar')
  })

  it('sem um único quadro, passado o prazo: «sem imagem»', () => {
    const v = vigiaNovo(T0)
    const r = proximoFluxo({ videoWidth: 0, currentTime: 0 }, v, T0 + 4000, RELOGIO)
    expect(r.fluxo).toBe('sem-imagem')
  })

  it('o primeiro quadro apaga a suspeita e marca que já houve imagem', () => {
    const v = vigiaNovo(T0)
    const r = proximoFluxo({ videoWidth: 1280, currentTime: 0.2 }, v, T0 + 9000, RELOGIO)
    expect(r.fluxo).toBe('a-chegar')
    expect(r.vigia.jaHouveImagem).toBe(true)
    expect(r.vigia.ultimoTempo).toBe(0.2)
  })

  it('com o tempo a avançar, nunca acusa — por muito que a chamada dure', () => {
    let v: Vigia = vigiaNovo(T0)
    let t = T0
    for (let i = 1; i <= 30; i++) {
      t += 1000
      const r = proximoFluxo({ videoWidth: 1280, currentTime: i }, v, t, RELOGIO)
      expect(r.fluxo).toBe('a-chegar')
      v = r.vigia
    }
  })

  it('imagem que PAROU usa o prazo longo, não o curto', () => {
    const v = proximoFluxo({ videoWidth: 1280, currentTime: 5 }, vigiaNovo(T0), T0, RELOGIO).vigia
    // 4 s parado: é o prazo do primeiro quadro, mas aqui já houve imagem.
    expect(proximoFluxo({ videoWidth: 1280, currentTime: 5 }, v, T0 + 4000, RELOGIO).fluxo).toBe('a-chegar')
    expect(proximoFluxo({ videoWidth: 1280, currentTime: 5 }, v, T0 + 6000, RELOGIO).fluxo).toBe('parado')
  })

  it('a imagem voltar limpa o estado de parada', () => {
    const v = proximoFluxo({ videoWidth: 1280, currentTime: 5 }, vigiaNovo(T0), T0, RELOGIO).vigia
    expect(proximoFluxo({ videoWidth: 1280, currentTime: 5 }, v, T0 + 7000, RELOGIO).fluxo).toBe('parado')
    const voltou = proximoFluxo({ videoWidth: 1280, currentTime: 6 }, v, T0 + 8000, RELOGIO)
    expect(voltou.fluxo).toBe('a-chegar')
    expect(voltou.vigia.mexeuEm).toBe(T0 + 8000)
  })

  it('o prazo é o do relógio que lhe derem — não há números escondidos', () => {
    const v = vigiaNovo(T0)
    const curto = { primeiro: 1000, parado: 2000 }
    expect(proximoFluxo({ videoWidth: 0, currentTime: 0 }, v, T0 + 999, curto).fluxo).toBe('a-chegar')
    expect(proximoFluxo({ videoWidth: 0, currentTime: 0 }, v, T0 + 1000, curto).fluxo).toBe('sem-imagem')
  })
})
