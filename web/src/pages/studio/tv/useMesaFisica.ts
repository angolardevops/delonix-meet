/**
 * Abrir o MIDI e pôr uma mesa física a comandar o estúdio.
 *
 * O que decide o que cada mensagem FAZ está no `studio/tv/mesaFisica.ts`, puro
 * e com teste. Aqui só vive o que precisa do browser: pedir o acesso, seguir os
 * dispositivos que entram e saem, e mandar as luzes de volta.
 *
 * O ACESSO PEDE-SE A UM CLIQUE, nunca sozinho: o `requestMIDIAccess` levanta um
 * pedido de permissão, e um estúdio que o levanta ao abrir ensina a pessoa a
 * recusar sem ler.
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  accaoDaMesaFisica,
  type AccaoFisica,
  descreverMidi,
  lerMidi,
  lucesDaMesa,
  midiSuportado,
} from '../../../studio/tv/mesaFisica'

export interface MesaFisica {
  suportado: boolean
  /** Os nomes das superfícies ligadas. Vazio = nenhuma. */
  dispositivos: string[]
  /** A última mensagem recebida, descrita — serve quem tem outra mesa. */
  ultima: string
  erro: string
  aLigar: boolean
  ligar: () => Promise<void>
}

export interface AlvoDaMesaFisica {
  /** Executa uma acção. Devolve os estados dos canais, para as luzes. */
  executar: (a: AccaoFisica) => void
  luzes: () => { mudo: boolean; solo: boolean }[]
}

export function useMesaFisica(alvo: AlvoDaMesaFisica): MesaFisica {
  const [dispositivos, setDispositivos] = useState<string[]>([])
  const [ultima, setUltima] = useState('')
  const [erro, setErro] = useState('')
  const [aLigar, setALigar] = useState(false)
  const acesso = useRef<MIDIAccess | null>(null)
  // Em refs: o alvo muda a cada render e o `onmidimessage` fica preso ao que
  // existia quando se ligou — era o fader a mexer num canal que já saiu.
  const refAlvo = useRef(alvo)
  refAlvo.current = alvo

  const ouvir = useCallback((a: MIDIAccess) => {
    const nomes: string[] = []
    for (const entrada of a.inputs.values()) {
      nomes.push(entrada.name || entrada.id)
      entrada.onmidimessage = (e) => {
        const m = lerMidi((e as MIDIMessageEvent).data ?? new Uint8Array())
        if (!m) return
        setUltima(descreverMidi(m))
        const accao = accaoDaMesaFisica(m)
        if (accao) refAlvo.current.executar(accao)
      }
    }
    setDispositivos(nomes)
  }, [])

  const ligar = useCallback(async () => {
    if (!midiSuportado()) {
      setErro('nao-suportado')
      return
    }
    setALigar(true)
    setErro('')
    try {
      const a = await navigator.requestMIDIAccess({ sysex: false })
      acesso.current = a
      // Uma superfície ligada a meio de uma emissão tem de entrar sozinha: é
      // o caso normal de quem chega ao estúdio com a mesa na mão.
      a.onstatechange = () => ouvir(a)
      ouvir(a)
    } catch {
      setErro('recusado')
    } finally {
      setALigar(false)
    }
  }, [ouvir])

  // As luzes seguem o estado da mesa do ecrã. Uma superfície sem luzes ignora
  // isto e não se parte nada.
  useEffect(() => {
    const a = acesso.current
    if (!a || !dispositivos.length) return
    const t = window.setInterval(() => {
      const mensagens = lucesDaMesa(refAlvo.current.luzes())
      for (const saida of a.outputs.values()) {
        for (const m of mensagens) {
          try {
            saida.send(m)
          } catch {
            // Uma superfície que fechou a meio não derruba o estúdio.
          }
        }
      }
    }, 250)
    return () => window.clearInterval(t)
  }, [dispositivos])

  useEffect(
    () => () => {
      const a = acesso.current
      acesso.current = null
      if (!a) return
      a.onstatechange = null
      for (const e of a.inputs.values()) e.onmidimessage = null
    },
    [],
  )

  return { suportado: midiSuportado(), dispositivos, ultima, erro, aLigar, ligar }
}
