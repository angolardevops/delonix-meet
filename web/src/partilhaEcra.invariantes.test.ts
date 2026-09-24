import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

// A partilha de ecrã vive agora no seu hook (`room/useScreenShare.ts`).
const share = readFileSync(join(__dirname, 'room', 'useScreenShare.ts'), 'utf8')
const webrtc = readFileSync(join(__dirname, 'webrtc.ts'), 'utf8')

/**
 * Parar de partilhar tem de PARAR a captura. É a única parte da sala onde um
 * defeito silencioso é um problema de privacidade e não de comodidade: a pessoa
 * carrega em «parar», acredita que parou, e o browser continua a ler o ecrã.
 */
describe('R111 · a partilha de ecrã pára mesmo quando pára', () => {
  const toggle = (() => {
    // O corpo da função, até à função seguinte do hook — e não até ao fim do
    // ficheiro, para que uma paragem noutro sítio não passe por esta.
    const i = share.indexOf('async function toggleShare()')
    expect(i).toBeGreaterThan(0)
    const fim = share.indexOf('\n  function ', i)
    expect(fim).toBeGreaterThan(i)
    return share.slice(i, fim)
  })()

  it('o caminho SFU pára as tracks do ecrã', () => {
    // Aqui o stream vive em `presentation`, e é de lá que se param as tracks.
    expect(toggle).toMatch(/presentation\?\.stream\.getTracks\(\)\.forEach\(\(t\) => t\.stop\(\)\)/)
  })

  it('o caminho MESH pára as tracks do ecrã', () => {
    // Aqui o stream NÃO vive em `presentation` — o vídeo entrou por
    // `replaceVideoTrack` e mais nada o guarda. Sem a referência própria, o
    // «parar partilha» do nosso botão não parava captura nenhuma.
    expect(toggle).toMatch(/displayStreamRef\.current\?\.getTracks\(\)\.forEach\(\(t\) => t\.stop\(\)\)/)
    expect(toggle).toMatch(/displayStreamRef\.current = display/)
  })

  it('o áudio do sistema que o mesh não publica é parado e explicado', () => {
    // O `SCREEN_CONSTRAINTS` PEDE áudio, por isso o browser mostra a caixa. Se
    // a pessoa a marca e nada acontece, é o consentimento vazio do R109 outra
    // vez — em ponto pequeno, mas a mesma coisa.
    expect(webrtc).toMatch(/audio: \{ echoCancellation: false/)
    expect(toggle).toMatch(/display\.getAudioTracks\(\)/)
    expect(toggle).toMatch(/setStatus\(t\('room\.txt\.audioDoSistemaSoEmSfu'\)\)/)
  })

  it('parar pelo botão do BROWSER também pára — e não reabre o selector', () => {
    // O `onended` chama a versão ACTUAL do `toggleShare` (pelo ref). Uma closure
    // presa ao render em que `sharing` era falso abria o selector outra vez em
    // vez de parar.
    expect(toggle).toMatch(/screenTrack\.onended = \(\) => toggleShareRef\.current\(\)/)
    expect(share).toMatch(/toggleShareRef\.current = \(\) => void toggleShare\(\)/)
  })

  it('sair da sala a partilhar não deixa o browser a capturar', () => {
    const i = share.indexOf('Sair da sala a partilhar')
    expect(i).toBeGreaterThan(0)
    expect(share.slice(i, i + 300)).toMatch(/displayStreamRef\.current\?\.getTracks\(\)\.forEach\(\(tr\) => tr\.stop\(\)\)/)
  })

  it('o caminho SFU continua a publicar o áudio do sistema', () => {
    // O que o mesh não faz, o SFU faz — e é o caminho por omissão. Se isto se
    // partir, a promessa «partilha de ecrã com áudio do sistema» passa a ser
    // falsa nos dois caminhos, e não em um.
    const i = webrtc.indexOf('async startScreen(track: MediaStreamTrack, stream: MediaStream)')
    expect(i).toBeGreaterThan(0)
    expect(webrtc.slice(i, i + 1200)).toMatch(/getAudioTracks\(\)\[0\]/)
  })
})
