/**
 * Invariantes do PWA offline.
 *
 * O comportamento é verificado com a rede REALMENTE cortada em
 * `e2e/offline.mjs`. Isto guarda as decisões que um `git revert` distraído
 * desfaz sem nada ficar vermelho.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')
const semComentarios = (p: string) =>
  read(p)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .filter((l) => !l.trimStart().startsWith('//'))
    .join('\n')

describe('a fronteira honesta: o que NÃO pode vir de cache', () => {
  const sw = semComentarios('web/public/sw.js')

  it('a API, o /ws e o /rtc nunca são servidos de cache', () => {
    // Uma app que dissesse «offline» e depois servisse uma lista de reuniões
    // velha, ou deixasse alguém pensar que entrou numa sala, estaria a mentir.
    expect(sw).toContain("url.pathname.startsWith('/api')")
    expect(sw).toContain("url.pathname === '/ws'")
    expect(sw).toContain("url.pathname === '/rtc'")
  })

  it('e o handler devolve sem responder, em vez de tentar a cache', () => {
    const linha = sw.split('\n').find((l) => l.includes("startsWith('/api')") && l.includes('return'))
    expect(linha).toBeTruthy()
  })
})

describe('o precache vem do GRAFO, não de nomes escritos à mão', () => {
  const cfg = semComentarios('web/vite.config.ts')

  it('há um fecho das importações estáticas', () => {
    // Escolher ficheiros por padrão de nome deixou de fora o `media-*.js` que
    // o Estúdio importa, e a rota rebentava offline. O grafo não se engana.
    expect(cfg).toContain('const fecho = (raizes: string[]): string[]')
    expect(cfg).toContain('pilha.push(...parte.imports)')
  })

  it('o Estúdio entra no precache — é o que se promete offline', () => {
    expect(cfg).toMatch(/Studio-\[\^\/\]\+/)
  })

  it('a Room e os modelos NÃO entram', () => {
    // A Room precisa do servidor por definição; os modelos passam dos 30 MB.
    expect(cfg).not.toMatch(/assets\\\/Room-/)
    expect(cfg).not.toContain('/models/')
  })

  it('a versão da cache vem do conteúdo, não de uma data', () => {
    // Uma data ou um contador fariam toda a gente descarregar tudo outra vez
    // a cada deploy, mesmo sem mudanças.
    expect(cfg).toContain("createHash('sha256')")
  })

  it('o plugin corre no writeBundle — o public/ não passa pelo Rollup', () => {
    expect(cfg).toContain('writeBundle(opcoes, bundle)')
    expect(cfg).not.toContain('generateBundle(_opcoes, bundle)')
  })
})

describe('o service worker regista-se onde deve', () => {
  const main = semComentarios('web/src/main.tsx')

  it('em qualquer contexto seguro, não só em https não-localhost', () => {
    // A condição antiga excluía `http://localhost` (que É contexto seguro) e
    // qualquer instalação self-hosted em HTTP numa rede interna.
    expect(main).toContain('window.isSecureContext')
    expect(main).not.toContain("location.protocol === 'https:'")
  })

  it('mas nunca no servidor de desenvolvimento', () => {
    // Um SW a guardar módulos do Vite dá uma app teimosa que serve código
    // velho e ninguém percebe porquê.
    expect(main).toContain('!import.meta.env.DEV')
  })
})

describe('o arquivo local', () => {
  const arq = semComentarios('web/src/studio/arquivo.ts')

  it('usa IndexedDB, não localStorage', () => {
    // localStorage guarda texto e anda pelos 5 MB; uma aula são centenas.
    expect(arq).toContain('indexedDB.open')
    expect(arq).not.toContain('localStorage')
  })

  it('larga os blobs depois de enviar', () => {
    // Guardar a aula depois de ela estar no servidor ocupa o disco do
    // utilizador duas vezes pela mesma coisa.
    expect(arq).toContain('enviada: true, completo: null, audio: null')
  })
})

describe('guardar é local PRIMEIRO', () => {
  it('o arquivo é escrito antes de se tentar o servidor', () => {
    // Procura DENTRO da função, não no ficheiro todo: o `enviarUma` também
    // chama `uploadRecording`, e uma busca global encontrava-o e dava verde a
    // uma ordem invertida. Foi o que a primeira versão deste teste fazia.
    const s = semComentarios('web/src/pages/Studio.tsx')
    const inicio = s.indexOf('async function guardarNaBiblioteca()')
    expect(inicio).toBeGreaterThan(-1)
    const corpo = s.slice(inicio, s.indexOf('\n  }', inicio))
    const iArquivo = corpo.indexOf('arquivo.guardar(')
    const iUpload = corpo.indexOf('uploadRecording(')
    expect(iArquivo).toBeGreaterThan(-1)
    expect(iUpload).toBeGreaterThan(-1)
    // Fazer o upload primeiro e guardar só em caso de falha perde a aula
    // quando o upload rebenta a meio — que é quando ela é mais precisa.
    expect(iUpload).toBeGreaterThan(iArquivo)
  })

  it('uma falha de upload não é apresentada como erro, mas como adiamento', () => {
    const s = semComentarios('web/src/pages/Studio.tsx')
    expect(s).toContain("setGuardado(t('studio.guardadoLocal'")
  })
})
