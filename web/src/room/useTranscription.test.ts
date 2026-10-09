import { describe, expect, it, vi } from 'vitest'

// `useTranscription.ts` importa `api.ts`, que lê o `localStorage` no topo do
// módulo — e a bateria corre em `node` (não há jsdom instalado neste repo).
// O esboço tem de existir ANTES do import, por isso o módulo entra por
// `await import()` e não por `import` estático — mesmo padrão de
// `api.guardas.test.ts`.
vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
  clear: () => {},
})
vi.stubGlobal('window', { dispatchEvent: () => true, addEventListener: () => {} })

const { devoTentarGravarActaDeReserva } = await import('./useTranscription')

// Não há infra-estrutura de teste de hooks React neste repo (sem
// @testing-library/react-hooks nem equivalente) — por isso testa-se a lógica
// PURA que decide se vale a pena tentar a gravação de reserva, extraída de
// `handleUnload`/`saveOnLeave`/`saveOnServerRecordingStop` em
// `useTranscription.ts`. A assinatura da função já prova o essencial: não
// recebe `isHost` nenhum — qualquer participante (anfitrião ou não) que seja
// o último a sair tenta gravar, não só o anfitrião.
describe('devoTentarGravarActaDeReserva', () => {
  it('um participante QUALQUER (não necessariamente anfitrião) tenta gravar quando há transcrição e ainda não se gravou', () => {
    // Simula o caso que falhava antes desta mudança: um convidado comum (não
    // anfitrião) é o último a sair, há linhas transcritas, e a acta ainda
    // não foi gravada por ninguém (`momSaved` falso). A função não tem como
    // saber se quem chama é o anfitrião — não lhe pergunta — por isso o
    // resultado é o mesmo para qualquer participante.
    expect(devoTentarGravarActaDeReserva(3, false)).toBe(true)
  })

  it('não tenta gravar sem nenhuma linha transcrita', () => {
    expect(devoTentarGravarActaDeReserva(0, false)).toBe(false)
  })

  it('não tenta gravar outra vez depois de já ter gravado', () => {
    expect(devoTentarGravarActaDeReserva(5, true)).toBe(false)
  })

  it('não tenta gravar sem linhas E já gravada', () => {
    expect(devoTentarGravarActaDeReserva(0, true)).toBe(false)
  })
})
