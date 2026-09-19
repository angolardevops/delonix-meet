import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const room = readFileSync(join(__dirname, 'pages', 'Room.tsx'), 'utf8')
const webrtc = readFileSync(join(__dirname, 'webrtc.ts'), 'utf8')

/**
 * Parar de partilhar tem de PARAR a captura. É a única parte da sala onde um
 * defeito silencioso é um problema de privacidade e não de comodidade: a pessoa
 * carrega em «parar», acredita que parou, e o browser continua a ler o ecrã.
 */
describe('R111 · a partilha de ecrã pára mesmo quando pára', () => {
  const toggle = (() => {
    const i = room.indexOf('async function toggleShare()')
    expect(i).toBeGreaterThan(0)
    return room.slice(i, room.indexOf('async function toggleRecording()', i))
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

  it('o caminho SFU continua a publicar o áudio do sistema', () => {
    // O que o mesh não faz, o SFU faz — e é o caminho por omissão. Se isto se
    // partir, a promessa «partilha de ecrã com áudio do sistema» passa a ser
    // falsa nos dois caminhos, e não em um.
    const i = webrtc.indexOf('async startScreen(track: MediaStreamTrack, stream: MediaStream)')
    expect(i).toBeGreaterThan(0)
    expect(webrtc.slice(i, i + 1200)).toMatch(/getAudioTracks\(\)\[0\]/)
  })
})
