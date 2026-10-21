/**
 * O link que o administrador entrega (B5) tem de abrir a página da reposição
 * (B4). São dois ficheiros que têm de concordar — o `passwordResetLink` da
 * `api.ts` e o `parseHash` da `rota.ts` — e nenhum build falha se um mudar
 * sozinho: o administrador entregaria um link que abre o Início, e a pessoa
 * continuava fora da conta.
 */
import { describe, expect, it, vi } from 'vitest'

// O `api.ts` lê o localStorage no topo do módulo e a bateria corre em `node`:
// o esboço tem de existir ANTES do import (ver `api.guardas.test.ts`).
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })
vi.stubGlobal('location', { origin: 'https://meet.exemplo.ao', pathname: '/', hash: '' })

const { passwordResetLink } = await import('./api')
const { parseHash } = await import('./rota')

describe('o link de reposição do administrador', () => {
  it('abre a página de reposição com o mesmo token', () => {
    const link = passwordResetLink('dlxr_0a9F')
    expect(link).toBe('https://meet.exemplo.ao/#/repor-password?token=dlxr_0a9F')
    const hash = link.slice(link.indexOf('#'))
    expect(parseHash(hash)).toEqual({ kind: 'repor-password', token: 'dlxr_0a9F' })
  })

  it('o token vai no fragmento, nunca no caminho nem na query', () => {
    const antes = passwordResetLink('dlxr_segredo').split('#')[0]
    expect(antes).not.toContain('dlxr_segredo')
  })
})
