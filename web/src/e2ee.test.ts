import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { deriveRoomKey } from './e2ee'

/**
 * O módulo que cifra a media não tinha UM teste.
 *
 * É a promessa de segurança que o produto mais destaca — «nem o SFU consegue
 * ver/ouvir» — e a lógica que a cumpre vive dentro de uma STRING (`WORKER_SRC`),
 * fora do alcance do TypeScript, do lint e, até aqui, dos testes. Uma string não
 * compila: um erro de sintaxe lá dentro só aparece quando alguém entra numa sala
 * E2EE.
 *
 * Estes testes avaliam essa string em Node — o MESMO código que corre no worker,
 * com a WebCrypto do Node, que é a mesma API. Não substituem um browser; provam
 * as decisões, que é onde os defeitos desta família vivem.
 */

// ── Extrair as funções da string do worker ──────────────────────────────────
//
// Não se copia o código para aqui. Um teste sobre uma CÓPIA prova que a cópia
// funciona, e é assim que se deixa de ver a divergência.
const fonte = readFileSync(join(__dirname, 'e2ee.ts'), 'utf8')
const inicio = fonte.indexOf('const WORKER_SRC = `') + 'const WORKER_SRC = `'.length
const fim = fonte.indexOf('\n`\n', inicio)
const src = fonte.slice(inicio, fim)
// O `onmessage`/`onrtctransform` do worker não existem em Node. Corta-se ali: o
// que interessa é o que decide sobre cada frame.
const corpo = src.slice(0, src.indexOf('onmessage ='))

type Frame = { data: ArrayBuffer; type?: string }
const fabrica = new Function(`
  ${corpo}
  return {
    definirChave: async (raw) => { key = await crypto.subtle.importKey('raw', raw, 'AES-GCM', false, ['encrypt','decrypt']) },
    esquecerChave: () => { key = null },
    cryptoOffset,
    encryptFrame,
    decryptFrame,
  }
`)
const w = fabrica() as {
  definirChave: (raw: ArrayBuffer) => Promise<void>
  esquecerChave: () => void
  cryptoOffset: (kind: string, type?: string) => number
  encryptFrame: (f: Frame, c: { enqueue: (f: Frame) => void }, kind: string) => Promise<void>
  decryptFrame: (f: Frame, c: { enqueue: (f: Frame) => void }, kind: string) => Promise<void>
}

const bytes = (n: number) => new Uint8Array(Array.from({ length: n }, (_, i) => (i * 7 + 3) & 0xff))
const coletor = () => {
  const saidos: Frame[] = []
  return { ctrl: { enqueue: (f: Frame) => saidos.push(f) }, saidos }
}

describe('e2ee · o offset do header', () => {
  it('vídeo keyframe = 10, delta = 3, áudio = 1', () => {
    // O header fica EM CLARO para os packetizers continuarem a funcionar. Se
    // este número mudar, os frames deixam de ser desempacotáveis — e o sintoma
    // é vídeo preto, não um erro.
    expect(w.cryptoOffset('video', 'key')).toBe(10)
    expect(w.cryptoOffset('video', 'delta')).toBe(3)
    expect(w.cryptoOffset('audio', undefined)).toBe(1)
  })
})

describe('e2ee · fail-closed', () => {
  it('sem chave, NADA sai — nem em claro', async () => {
    // A garantia que interessa: numa sala marcada E2EE, media em claro a sair é
    // uma quebra silenciosa de confidencialidade. Sem media é um sintoma
    // visível, e um sintoma visível é sempre melhor.
    w.esquecerChave()
    const { ctrl, saidos } = coletor()
    await w.encryptFrame({ data: bytes(120).buffer, type: 'key' }, ctrl, 'video')
    expect(saidos).toHaveLength(0)
  })

  it('sem chave, nada é entregue ao descodificador', async () => {
    w.esquecerChave()
    const { ctrl, saidos } = coletor()
    await w.decryptFrame({ data: bytes(120).buffer, type: 'key' }, ctrl, 'video')
    expect(saidos).toHaveLength(0)
  })
})

describe('e2ee · a ida e a volta', () => {
  it('cifrar e decifrar devolve os MESMOS bytes', async () => {
    const chave = await deriveRoomKey('uma frase combinada', 'sala-teste')
    await w.definirChave(chave)
    const original = bytes(120)
    const a = coletor()
    await w.encryptFrame({ data: original.buffer.slice(0), type: 'key' }, a.ctrl, 'video')
    expect(a.saidos).toHaveLength(1)
    const cifrado = new Uint8Array(a.saidos[0].data)
    // Cresce: ciphertext + tag GCM (16) + IV (12).
    expect(cifrado.byteLength).toBe(original.byteLength + 16 + 12)
    // O payload NÃO é o original — se fosse, isto não cifrava nada.
    expect([...cifrado.slice(10, 40)]).not.toEqual([...original.slice(10, 40)])
    // E o header fica em claro, que é a razão de ser do offset.
    expect([...cifrado.slice(0, 10)]).toEqual([...original.slice(0, 10)])

    const b = coletor()
    await w.decryptFrame({ data: a.saidos[0].data, type: 'key' }, b.ctrl, 'video')
    expect(b.saidos).toHaveLength(1)
    expect([...new Uint8Array(b.saidos[0].data)]).toEqual([...original])
  })

  it('a MESMA frase e sala dão a mesma chave; sala diferente dá outra', async () => {
    // O código da sala é o SAL. Sem ele, a mesma frase-chave usada em duas
    // reuniões daria a mesma chave, e gravar uma serviria para abrir a outra.
    const [k1, k2, k3] = await Promise.all([
      deriveRoomKey('frase', 'sala-a'),
      deriveRoomKey('frase', 'sala-a'),
      deriveRoomKey('frase', 'sala-b'),
    ])
    const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map((x) => x.toString(16)).join('')
    expect(hex(k1)).toBe(hex(k2))
    expect(hex(k1)).not.toBe(hex(k3))
    expect(new Uint8Array(k1).byteLength).toBe(32)
  })
})

describe('e2ee · o que NÃO autentica é descartado', () => {
  it('a frase errada não decifra — e não entrega ruído ao descodificador', async () => {
    await w.definirChave(await deriveRoomKey('a frase certa', 'sala'))
    const a = coletor()
    await w.encryptFrame({ data: bytes(120).buffer, type: 'key' }, a.ctrl, 'video')

    await w.definirChave(await deriveRoomKey('a frase errada', 'sala'))
    const b = coletor()
    await w.decryptFrame({ data: a.saidos[0].data, type: 'key' }, b.ctrl, 'video')
    expect(b.saidos).toHaveLength(0)
  })

  it('mexer no HEADER em claro invalida o frame — é o additionalData a servir', async () => {
    // O header vai em claro para os packetizers, mas AUTENTICADO. Sem isso, um
    // intermediário podia trocar o tipo de frame sem que nada desse por isso —
    // e é essa a razão de ele ir como `additionalData`.
    await w.definirChave(await deriveRoomKey('frase', 'sala'))
    const a = coletor()
    await w.encryptFrame({ data: bytes(120).buffer, type: 'key' }, a.ctrl, 'video')
    const adulterado = new Uint8Array(a.saidos[0].data)
    adulterado[2] ^= 0xff
    const b = coletor()
    await w.decryptFrame({ data: adulterado.buffer, type: 'key' }, b.ctrl, 'video')
    expect(b.saidos).toHaveLength(0)
  })

  it('um frame demasiado pequeno não sai em claro', async () => {
    // Abaixo do offset não há payload a cifrar. Deixá-lo passar expunha-o sem
    // sequer servir: o receptor descartá-lo-ia na mesma, com um limiar maior.
    await w.definirChave(await deriveRoomKey('frase', 'sala'))
    const { ctrl, saidos } = coletor()
    await w.encryptFrame({ data: bytes(8).buffer, type: 'key' }, ctrl, 'video')
    expect(saidos).toHaveLength(0)
  })

  it('um frame com EXACTAMENTE o tamanho do header também não sai', async () => {
    // A fronteira, e ela é `<=` e não `<`: um keyframe de exactamente 10 bytes
    // é só header — não há payload nenhum a cifrar. Com `<` sairia um frame com
    // ciphertext vazio, tag e IV: 28 bytes de nada, que o receptor descarta.
    //
    // Este caso apareceu A SABOTAR: com oito bytes o teste passava nas duas
    // versões da condição, e a troca de `<=` por `<` sobrevivia a todos os
    // outros oito testes deste ficheiro.
    await w.definirChave(await deriveRoomKey('frase', 'sala'))
    const { ctrl, saidos } = coletor()
    await w.encryptFrame({ data: bytes(10).buffer, type: 'key' }, ctrl, 'video')
    expect(saidos).toHaveLength(0)
  })

  it('um frame curto de mais para conter tag+IV é descartado à chegada', async () => {
    await w.definirChave(await deriveRoomKey('frase', 'sala'))
    const { ctrl, saidos } = coletor()
    await w.decryptFrame({ data: bytes(30).buffer, type: 'key' }, ctrl, 'video')
    expect(saidos).toHaveLength(0)
  })
})
