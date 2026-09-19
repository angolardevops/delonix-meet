import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { AGENTE_CONTROLO_REMOTO } from './capabilities'

const room = readFileSync(join(__dirname, 'pages', 'Room.tsx'), 'utf8')

/**
 * O produto pode PROMETER o que ainda não faz — num roadmap, e sem `done`. O
 * que não pode é DIZER QUE FEZ. Este portão guarda o caso que aconteceu a
 * sério: o controlo remoto anunciava-se «ativo» com zero linhas a encaminhar um
 * clique, e o dono do ecrã dava um consentimento que não queria dizer nada.
 */
describe('R109 · nenhuma capacidade se anuncia activa sem código por trás', () => {
  it('o controlo remoto só se pede se houver agente que o receba', () => {
    // O `undefined` é o que esconde o botão: o `PresentationTile` só o desenha
    // quando recebe um callback.
    expect(room).toMatch(/AGENTE_CONTROLO_REMOTO\s*\n?\s*\?\s*\(\) => signalRef/)
    expect(room).toMatch(/:\s*undefined/)
  })

  it('um pedido que chegue sem agente é recusado, não posto a votos', () => {
    // Sem isto, um cliente antigo (ou um curioso a mandar a mensagem à mão)
    // abria o diálogo de consentimento na cara de quem partilha o ecrã.
    const i = room.indexOf("if (m.action === 'request')")
    expect(i).toBeGreaterThan(0)
    const bloco = room.slice(i, i + 800)
    expect(bloco).toMatch(/if \(!AGENTE_CONTROLO_REMOTO\)/)
    // e a recusa acontece ANTES de `setCtrlAsk` — a ordem é o que importa.
    expect(bloco.indexOf('AGENTE_CONTROLO_REMOTO')).toBeLessThan(bloco.indexOf('setCtrlAsk'))
  })

  it('a mensagem de «controlo ativo» é inalcançável enquanto não houver agente', () => {
    // Se alguém ligar a bandeira sem construir o agente, este teste passa a
    // exigir que o encaminhamento exista de verdade — e não passa.
    if (!AGENTE_CONTROLO_REMOTO) return
    // Um agente a sério tem de encaminhar ALGUMA coisa. Se a bandeira estiver
    // ligada, tem de haver envio de eventos de input em algum lado.
    expect(room).toMatch(/remote-control-input|controlInput|sendPointer|sendKey/)
  })
})
