/**
 * Fitness functions do lote 3 (docs/ux-perf-review.md).
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')
const sala = () => read('web/src/pages/Room.tsx')

describe('2.1 · nenhum relógio bate na raiz da sala', () => {
  it('não há setState de tempo no componente Room', () => {
    for (const proibido of ['setElapsed(', 'setPollNow(', 'setNow(', 'setClock(']) {
      expect(sala()).not.toContain(proibido)
    }
  })

  it('os relógios vivem em folhas próprias', () => {
    const c = read('web/src/room/Clocks.tsx')
    for (const f of ['export function MeetingElapsed', 'export function Countdown', 'export function WallClock']) {
      expect(c).toContain(f)
    }
    expect(sala()).toContain("from '../room/Clocks'")
  })

  it('a duração ainda sabe DE ONDE conta', () => {
    // A extracção dos relógios levou consigo o efeito que escrevia
    // `joinedAtRef` e deixou ficar a declaração e o uso. O contador passou a
    // contar desde 1970 — mostrava `496594:12:29` em vez de `00:37`.
    //
    // Os testes acima não deram por nada: verificavam que o relógio SAIU da
    // raiz, não que continuava a saber quando a reunião começou. É essa a
    // metade que faltava.
    expect(sala()).toContain('joinedAtRef.current = Date.now()')
    // E a folha recusa-se a inventar um número quando não lhe dizem a hora.
    expect(read('web/src/room/Clocks.tsx')).toContain('if (!startedAt) return null')
  })

  it('o fecho automático de sondagens tica sem renderizar', () => {
    // Precisa do TIQUE, não de um render: lê o relógio do sistema dentro do
    // próprio intervalo e dispara. Se voltar a depender de estado, a sala
    // volta a reconciliar 86 400 vezes por dia.
    expect(sala()).toContain('const agora = Math.floor(Date.now() / 1000)')
    expect(sala()).toContain('pollsRef.current')
  })
})

describe('2.2 e 2.3 · os mosaicos não voltam a renderizar à toa', () => {
  const tile = () => read('web/src/room/RemoteTile.tsx')

  it('o mosaico é memoizado com comparação explícita', () => {
    expect(tile()).toContain('export const RemoteTile = memo(RemoteTileBase,')
    // Igualdade rasa não serve: `peer` é um objecto novo a cada actualização
    // de lista mesmo quando nada mudou.
    expect(tile()).toContain('a.peer.peerId === b.peer.peerId')
    expect(tile()).toContain('a.peer.stream === b.peer.stream')
  })

  it('os callbacks passados ao mosaico são estáveis', () => {
    // Uma closure nova por peer e por render anula qualquer memo a jusante.
    expect(sala()).toContain('const onTilePin = useCallback')
    expect(sala()).toContain('onPin={onTilePin}')
    expect(sala()).not.toMatch(/onMute=\{\(\) => signalRef/)
  })
})

describe('3.2.1 · o dashboard deixa de se resolver pela posição no ficheiro', () => {
  const css = () => read('web/src/styles.scss')
  for (const sel of ['.dash-card', '.dash-grid', '.dash-card-head']) {
    it(`${sel} está definido uma vez só ao nível de topo`, () => {
      // Só regras na COLUNA 0. Uma regra indentada está dentro de um
      // `@media` ou de um `[data-theme]`, e essa é uma variação legítima —
      // não é o problema do 3.2.1, que é a mesma regra declarada duas vezes
      // no mesmo âmbito e resolvida por quem aparece mais abaixo.
      const n = css().split('\n').filter((l) => l.startsWith(`${sel} {`)).length
      expect(n).toBe(1)
    })
  }
})

describe('3.2.4 · a marca não aparece em hexadecimal solto', () => {
  it('sem #eda33b nem #c8201d fora de fallbacks de var()', () => {
    const soltos = read('web/src/styles.scss')
      .split('\n')
      .filter((l) => /#(eda33b|c8201d)/i.test(l) && !/var\(--[a-z0-9-]+,\s*#/i.test(l))
    expect(soltos).toEqual([])
  })
})
