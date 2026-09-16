/**
 * Fitness functions do lote 3 (docs/ux-perf-review.md).
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')
const sala = () => read('web/src/pages/Room.tsx')

/**
 * A sala reconstruída tem a raiz dividida em hooks (`room/use*.ts`): todos
 * correm DENTRO do componente `Room`, por isso um relógio num deles bate na
 * raiz tanto como bateria na própria página. O portão lê a página e todos os
 * hooks.
 */
const raizDaSala = () =>
  [
    sala(),
    ...readdirSync(join(root, 'web/src/room'))
      .filter((f) => /^use[A-Z].*\.ts$/.test(f))
      .map((f) => read(`web/src/room/${f}`)),
  ].join('\n')

describe('2.1 · nenhum relógio bate na raiz da sala', () => {
  it('não há setState de tempo no componente Room nem nos hooks que ele corre', () => {
    for (const proibido of ['setElapsed(', 'setPollNow(', 'setNow(', 'setClock(']) {
      expect(raizDaSala()).not.toContain(proibido)
    }
  })

  it('os relógios vivem em folhas próprias', () => {
    const c = read('web/src/room/Clocks.tsx')
    for (const f of ['export function MeetingElapsed', 'export function Countdown', 'export function WallClock']) {
      expect(c).toContain(f)
    }
    // Quem os mostra importa-os — a barra de topo e a de controlos.
    expect(read('web/src/room/TopBar.tsx')).toContain("from './Clocks'")
    expect(read('web/src/room/ControlBar.tsx')).toContain("from './Clocks'")
    expect(read('web/src/room/TopBar.tsx')).toContain('<MeetingElapsed startedAt={joinedAt}')
    // Conta desde o início da SESSÃO no servidor (`joined.started_at`, lote 2);
    // sem ele, desde a minha entrada — nunca desde zero.
    expect(sala()).toContain('joinedAt={core.startedAtRef.current || core.joinedAtRef.current}')
  })

  it('a duração ainda sabe DE ONDE conta', () => {
    // A extracção dos relógios levou consigo o efeito que escrevia
    // `joinedAtRef` e deixou ficar a declaração e o uso. O contador passou a
    // contar desde 1970 — mostrava `496594:12:29` em vez de `00:37`.
    //
    // Os testes acima não deram por nada: verificavam que o relógio SAIU da
    // raiz, não que continuava a saber quando a reunião começou. É essa a
    // metade que faltava.
    expect(read('web/src/room/useRoomCore.ts')).toContain('joinedAtRef.current = Date.now()')
    // E a folha recusa-se a inventar um número quando não lhe dizem a hora.
    expect(read('web/src/room/Clocks.tsx')).toContain('if (!startedAt) return null')
  })

  it('o fecho automático de sondagens tica sem renderizar', () => {
    // Precisa do TIQUE, não de um render: lê o relógio do sistema dentro do
    // próprio intervalo e dispara. Se voltar a depender de estado, a sala
    // volta a reconciliar 86 400 vezes por dia.
    const t = read('web/src/room/useMeetingTools.ts')
    const i = t.indexOf('setInterval(() => {\n      const agora')
    expect(i).toBeGreaterThan(0)
    const tique = t.slice(i, i + 500)
    expect(tique).toContain('pollsRef.current')
    expect(tique).toContain("signal.send({ type: 'poll-close'")
    // O `ends_at` das sondagens vem em MILISSEGUNDOS (`now_ms()` do servidor).
    // A versão anterior comparava-o com `Math.floor(Date.now() / 1000)` e o
    // fecho automático nunca disparava — o portão antigo exigia essa linha.
    expect(tique).toContain('const agora = Date.now()')
    expect(read('server/src/room_tools.rs')).toMatch(/\.map\(\|d\| now_ms\(\) \+ \(d\.min\(3600\) as i64\) \* 1000\)/)
  })
})

describe('2.2 e 2.3 · os mosaicos não voltam a renderizar à toa', () => {
  const tile = () => read('web/src/room/ParticipantTile.tsx')

  it('o mosaico é memoizado com comparação explícita', () => {
    expect(tile()).toContain('export const ParticipantTile = memo(ParticipantTileBase,')
    // Igualdade rasa não serve: `peer` é um objecto novo a cada actualização
    // de lista mesmo quando nada mudou.
    expect(tile()).toContain('a.peer.peerId === b.peer.peerId')
    expect(tile()).toContain('a.peer.stream === b.peer.stream')
    // As dimensões chegam como NÚMEROS: um objecto `style` novo por render
    // anulava a comparação.
    expect(tile()).toContain('a.width === b.width')
  })

  it('os callbacks passados ao mosaico são estáveis', () => {
    // Uma closure nova por peer e por render anula qualquer memo a jusante.
    expect(sala()).toContain('const onTilePin = useCallback')
    expect(sala()).toContain('onTilePin={onTilePin}')
    const palco = read('web/src/room/Stage.tsx')
    expect(palco).toContain('onPin={onTilePin}')
    expect(palco).toContain('onMute={onTileMute}')
    expect(palco).toContain('onKick={onTileKick}')
    expect(palco).not.toMatch(/on(Pin|Mute|Kick)=\{\(/)
    // E os de silenciar/remover saem de `useCallback` no hook dos participantes.
    const p = read('web/src/room/useParticipants.ts')
    expect(p).toMatch(/const mute = useCallback\(/)
    expect(p).toMatch(/const kick = useCallback\(/)
  })
})

/** As folhas de estilo da app, derivadas da árvore. */
function folhas(dir = 'web/src'): string[] {
  const out: string[] = []
  for (const e of readdirSync(join(root, dir), { withFileTypes: true })) {
    const p = `${dir}/${e.name}`
    if (e.isDirectory()) out.push(...folhas(p))
    else if (e.name.endsWith('.css')) out.push(p)
  }
  return out.sort()
}

describe('3.2.1 · nenhuma regra se resolve pela posição no ficheiro', () => {
  // O defeito original: `.dash-card` declarado duas vezes no mesmo âmbito, e
  // quem ganhava era quem aparecia mais abaixo. Só contam regras na COLUNA 0 —
  // uma regra indentada está dentro de um `@media`, e essa variação é legítima.
  for (const f of folhas()) {
    it(`${f} não repete um selector ao nível de topo`, () => {
      // O selector é o grupo INTEIRO («a,\nb {»): uma regra de grupo seguida de
      // um refinamento de um dos membros é uma variação, não uma repetição.
      const vistos = new Map<string, number>()
      let grupo: string[] = []
      for (const l of read(f).split('\n')) {
        if (/^[.#\[:a-z][^{}@/]*,\s*$/i.test(l)) {
          grupo.push(l.trim().replace(/,$/, ''))
          continue
        }
        const m = l.match(/^([.#\[:a-z][^{}@/]*?)\s*\{\s*$/i)
        if (m) {
          const sel = [...grupo, m[1]].join(', ')
          vistos.set(sel, (vistos.get(sel) ?? 0) + 1)
        }
        grupo = []
      }
      expect([...vistos].filter(([, n]) => n > 1).map(([s]) => s)).toEqual([])
    })
  }
})

describe('3.2.4 · a marca não aparece em hexadecimal solto', () => {
  it('o vermelho Delonix só existe como token', () => {
    const soltos: string[] = []
    for (const f of folhas()) {
      if (f.endsWith('/ui/tokens.css')) continue
      read(f).split('\n').forEach((l, i) => {
        if (/#(e8232b|eda33b|c8201d)/i.test(l)) soltos.push(`${f}:${i + 1}`)
      })
    }
    expect(soltos).toEqual([])
  })
})
