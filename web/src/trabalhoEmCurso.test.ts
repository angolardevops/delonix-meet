/**
 * O store de «há media em curso». Pequeno de propósito — o que se prova aqui é
 * que ele avisa quem subscreveu e que não avisa por nada.
 */
import { describe, expect, it, vi } from 'vitest'
import {
  haTrabalhoEmCurso,
  marcarTrabalhoEmCurso,
  subscreverTrabalhoEmCurso,
} from './trabalhoEmCurso'

describe('trabalhoEmCurso', () => {
  it('começa falso e segue o que lhe dizem', () => {
    marcarTrabalhoEmCurso(false)
    expect(haTrabalhoEmCurso()).toBe(false)
    marcarTrabalhoEmCurso(true)
    expect(haTrabalhoEmCurso()).toBe(true)
    marcarTrabalhoEmCurso(false)
    expect(haTrabalhoEmCurso()).toBe(false)
  })

  it('avisa quem subscreveu, e SÓ quando o valor muda', () => {
    marcarTrabalhoEmCurso(false)
    const f = vi.fn()
    const cancelar = subscreverTrabalhoEmCurso(f)
    marcarTrabalhoEmCurso(true)
    expect(f).toHaveBeenCalledTimes(1)
    // O mesmo valor outra vez não é um aviso: senão cada render do Estúdio
    // acordava a paleta de comandos.
    marcarTrabalhoEmCurso(true)
    expect(f).toHaveBeenCalledTimes(1)
    marcarTrabalhoEmCurso(false)
    expect(f).toHaveBeenCalledTimes(2)
    cancelar()
    marcarTrabalhoEmCurso(true)
    expect(f).toHaveBeenCalledTimes(2)
    marcarTrabalhoEmCurso(false)
  })

  it('cancelar duas vezes não estoura, e os outros continuam a ser avisados', () => {
    marcarTrabalhoEmCurso(false)
    const a = vi.fn()
    const b = vi.fn()
    const cancelarA = subscreverTrabalhoEmCurso(a)
    const cancelarB = subscreverTrabalhoEmCurso(b)
    cancelarA()
    cancelarA()
    marcarTrabalhoEmCurso(true)
    expect(a).not.toHaveBeenCalled()
    expect(b).toHaveBeenCalledTimes(1)
    cancelarB()
    marcarTrabalhoEmCurso(false)
  })
})
