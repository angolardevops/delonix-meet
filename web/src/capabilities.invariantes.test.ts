import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { AGENTE_CONTROLO_REMOTO } from './capabilities'

const ler = (...p: string[]) => readFileSync(join(__dirname, ...p), 'utf8')
// A sala foi reconstruída: a lógica do controlo remoto vive no seu hook, e o
// botão no mosaico da apresentação. O portão lê os DOIS — e a página que os liga.
const hook = ler('room', 'useRemoteControl.ts')
const tile = ler('room', 'PresentationTile.tsx')
const room = ler('pages', 'Room.tsx')
const salaInteira = [room, ...readdirSync(join(__dirname, 'room')).filter((f) => /\.tsx?$/.test(f) && !f.includes('.test.')).map((f) => ler('room', f))].join('\n')

/**
 * O produto pode PROMETER o que ainda não faz — num roadmap, e sem `done`. O
 * que não pode é DIZER QUE FEZ. Este portão guarda o caso que aconteceu a
 * sério: o controlo remoto anunciava-se «ativo» com zero linhas a encaminhar um
 * clique, e o dono do ecrã dava um consentimento que não queria dizer nada.
 */
describe('R109 · nenhuma capacidade se anuncia activa sem código por trás', () => {
  it('o controlo remoto só se pede se houver agente que o receba', () => {
    // O `undefined` é o que esconde o botão: o mosaico da apresentação só o
    // desenha quando recebe um callback.
    expect(hook).toMatch(/AGENTE_CONTROLO_REMOTO\s*\n?\s*\?\s*\(\) => signal\.send/)
    expect(hook).toMatch(/:\s*undefined/)
    expect(room).toMatch(/onRequestControl=\{remote\.requestControl\}/)
    expect(tile).toMatch(/onRequestControl && \(/)
  })

  it('um pedido que chegue sem agente é recusado, não posto a votos', () => {
    // Sem isto, um cliente antigo (ou um curioso a mandar a mensagem à mão)
    // abria o diálogo de consentimento na cara de quem partilha o ecrã.
    const i = hook.indexOf("if (m.action === 'request')")
    expect(i).toBeGreaterThan(0)
    const bloco = hook.slice(i, i + 800)
    expect(bloco).toMatch(/if \(!AGENTE_CONTROLO_REMOTO\)/)
    // A recusa é ENVIADA, e acontece ANTES de `setCtrlAsk` — a ordem é o que importa.
    expect(bloco).toMatch(/action: 'deny'/)
    expect(bloco.indexOf('AGENTE_CONTROLO_REMOTO')).toBeLessThan(bloco.indexOf('setCtrlAsk'))
  })

  it('a mensagem de «controlo ativo» é inalcançável enquanto não houver agente', () => {
    // Se alguém ligar a bandeira sem construir o agente, este teste passa a
    // exigir que o encaminhamento exista de verdade — e não passa.
    if (!AGENTE_CONTROLO_REMOTO) return
    expect(salaInteira).toMatch(/remote-control-input|controlInput|sendPointer|sendKey/)
  })
})
