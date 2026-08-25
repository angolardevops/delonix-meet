/**
 * Perfil dos mosaicos da sala — o que o lote 3 devia ter medido ANTES de mexer.
 *
 * Não conduz a sala inteira (precisaria do SFU, de media e de pares reais).
 * Mede o que a correcção mudou: o CUSTO de a raiz voltar a renderizar sem que
 * os mosaicos tenham mudado — que é exactamente o que os três relógios de 1 Hz
 * provocavam, 86 400 vezes por dia.
 *
 * O instrumento é o `<Profiler>` do React: `actualDuration` é o tempo que o
 * commit gastou mesmo. Quando o `memo` faz bail-out, esse tempo cai — e é isso
 * que se quer ver. Um contador de renders num invólucro mediria o invólucro.
 *
 * O mosaico é o do produto (`src/room/RemoteTile.tsx`), não um duplo.
 */
import { Profiler, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { RemoteTile, RemoteTileBase, type RemotePeer } from '../../src/room/RemoteTile'

const q = new URLSearchParams(location.search)
const N = Number(q.get('n') ?? 12)
const TIQUES = Number(q.get('t') ?? 60)
const MEMO = q.get('memo') !== 'off'

// `memo` ligado = o componente do produto. Desligado = o mesmo componente sem
// a barreira, que é como estava antes do lote 3.
const Mosaico = MEMO ? RemoteTile : RemoteTileBase

const pares: RemotePeer[] = Array.from({ length: N }, (_, i) => ({
  peerId: `p${i}`, username: `Pessoa ${i}`, host: i === 0, hand: false,
  camOn: false, micOn: true, canAdmit: false, stream: null,
}))

// Callbacks estáveis, como a sala passou a fazer (achado 2.3).
const onPin = () => {}
const onMute = () => {}
const onKick = () => {}
const estilo = { width: 160, height: 90 }

let somaMs = 0
let commits = 0

function Sala() {
  // O relógio que o lote 3 tirou da raiz, simulado para medir o custo que TINHA.
  const [tique, setTique] = useState(0)
  useEffect(() => {
    if (tique >= TIQUES) {
      ;(window as unknown as { __bench?: unknown }).__bench = {
        modo: MEMO ? 'com memo' : 'sem memo',
        pares: N, tiques: TIQUES, commits,
        msTotal: Math.round(somaMs * 100) / 100,
        msPorTique: Math.round((somaMs / TIQUES) * 1000) / 1000,
      }
      return
    }
    const id = requestAnimationFrame(() => setTique((t) => t + 1))
    return () => cancelAnimationFrame(id)
  }, [tique])

  return (
    <Profiler
      id="mosaicos"
      onRender={(_id, fase, actualDuration) => {
        if (fase === 'update') { somaMs += actualDuration; commits++ }
      }}
    >
      <span style={{ position: 'absolute', opacity: 0 }}>{tique}</span>
      <div style={{ display: 'flex', flexWrap: 'wrap' }}>
        {pares.map((p) => (
          <Mosaico
            key={p.peerId}
            peer={p}
            isHost
            speaking={false}
            style={estilo}
            pinned={false}
            onPin={onPin}
            onMute={onMute}
            onKick={onKick}
          />
        ))}
      </div>
    </Profiler>
  )
}

createRoot(document.getElementById('raiz')!).render(<Sala />)
