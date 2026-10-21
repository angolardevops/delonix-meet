import { describe, expect, it, vi } from 'vitest'
import { copiarTexto, type AmbienteDeCopia } from './copy'

describe('copiarTexto', () => {
  it('usa a API moderna quando ela existe', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    const execCopy = vi.fn().mockReturnValue(true)
    expect(await copiarTexto('abc', { clipboard: { writeText }, execCopy })).toBe(true)
    expect(writeText).toHaveBeenCalledWith('abc')
    expect(execCopy).not.toHaveBeenCalled()
  })

  /**
   * O DEFEITO QUE ISTO GUARDA: o `navigator.clipboard` não existe fora de
   * contexto seguro, e o laboratório corre em http numa LAN. Nos oito sítios
   * antigos, carregar em «copiar» não fazia NADA — e dois engoliam a falha em
   * silêncio. Quem copia o PIN de um ramal, que aparece uma vez, não tinha
   * como saber.
   */
  it('em http (sem clipboard) cai no caminho de trás', async () => {
    const execCopy = vi.fn().mockReturnValue(true)
    expect(await copiarTexto('o-pin', { execCopy })).toBe(true)
    expect(execCopy).toHaveBeenCalledWith('o-pin')
  })

  it('a API que LANÇA também cai no caminho de trás', async () => {
    // Permissão negada ou documento sem foco: existe mas não serve.
    const writeText = vi.fn().mockRejectedValue(new Error('NotAllowedError'))
    const execCopy = vi.fn().mockReturnValue(true)
    expect(await copiarTexto('x', { clipboard: { writeText }, execCopy })).toBe(true)
    expect(execCopy).toHaveBeenCalled()
  })

  it('sem nenhum caminho diz FALSE em vez de fingir', async () => {
    const amb: AmbienteDeCopia = {}
    expect(await copiarTexto('x', amb)).toBe(false)
  })

  it('o caminho de trás que falha não mente', async () => {
    const writeText = vi.fn().mockRejectedValue(new Error('no'))
    expect(await copiarTexto('x', { clipboard: { writeText }, execCopy: () => false })).toBe(false)
  })
})
