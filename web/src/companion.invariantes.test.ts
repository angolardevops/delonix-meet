import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const ler = (...p: string[]) => readFileSync(join(__dirname, ...p), 'utf8')
// A sala foi reconstruída em hooks + vista. Cada asserção lê o ficheiro onde
// o comportamento vive AGORA: a sessão decide, a página liga o áudio, o
// `AudioSink` silencia, os avisos explicam.
const room = ler('pages', 'Room.tsx')
const sessao = ler('room', 'useCallSession.ts')
const sink = ler('room', 'AudioSink.tsx')
const avisos = ler('room', 'Notices.tsx')
const rust = readFileSync(join(__dirname, '..', '..', 'server', 'src', 'signaling.rs'), 'utf8')

/**
 * Entrar duas vezes na mesma reunião — portátil e telemóvel — é útil e é o que
 * o Meet chama companion mode. O que estraga a reunião é o eco: dois microfones
 * e dois altifalantes da mesma pessoa no mesmo espaço físico realimentam-se, e
 * o problema não é de quem o causa, é de toda a gente.
 */
describe('R114 · a segunda sessão da mesma conta entra sem áudio', () => {
  it('quem decide é o SERVIDOR — o cliente não sabe que a outra sessão é dele', () => {
    // Uma tentativa no cliente («já vi este nome no roster») falharia com dois
    // homónimos e falharia sempre com o nome mudado.
    expect(rust).toMatch(/\.any\(\|p\| p\.user_id == user_id && p\.disconnected_at\.is_none\(\)\)/)
    expect(sessao).toMatch(/if \(m\.companion\) \{/)
  })

  it('entra mudo nos DOIS sentidos', () => {
    // Só calar o microfone não chega: o altifalante deste dispositivo a tocar o
    // som da reunião ao pé do microfone do outro fecha o ciclo na mesma.
    const i = sessao.indexOf('if (m.companion) {')
    expect(i).toBeGreaterThan(0)
    expect(sessao.slice(i, i + 400)).toMatch(/mic\.enabled = false/)
    expect(sessao.slice(i, i + 400)).toMatch(/setMicOn\(false\)/)
    // O `companion` que silencia o áudio é o MESMO que a sessão liga.
    expect(room).toMatch(/const \{ companion \} = session/)
    // (o `volume` é o do deslizador deste dispositivo; o silêncio do companion ganha-lhe)
    expect(room).toMatch(/<AudioSink peers=\{peers\} sinkId=\{speakerId\} mudo=\{companion\} volume=\{media\.outputVolume\} \/>/)
  })

  it('o áudio é silenciado, não desmontado', () => {
    // Mesma razão do próprio `AudioSink` (o que se ouve não depende do layout):
    // o elemento fica ligado ao stream para que ligar o som seja instantâneo.
    expect(sink).toMatch(/<audio ref=\{ref\} autoPlay muted=\{mudo\} \/>/)
  })

  it('a pessoa fica a saber porquê, e pode decidir o contrário', () => {
    // Um dispositivo mudo sem explicação é indistinguível de um produto
    // partido — e é a queixa que se recebe, não a causa.
    expect(avisos).toMatch(/\{companion && \(/)
    expect(avisos).toMatch(/t\('room\.companion\.explicacao'\)/)
    expect(avisos).toMatch(/t\('room\.companion\.usarAudioAqui'\)/)
    expect(room).toMatch(/companion=\{companion\}/)
    expect(room).toMatch(/onUseAudioHere=\{session\.dismissCompanion\}/)
    // e ao ligar o áudio aqui, diz o que fazer ao outro dispositivo
    const i = sessao.indexOf('function dismissCompanion()')
    expect(i).toBeGreaterThan(0)
    expect(sessao.slice(i, i + 400)).toMatch(/t\('room\.companion\.silenciaOOutro'\)/)
  })

  it('e desliga-se sozinho quando o outro dispositivo sai', () => {
    // Uma funcionalidade que se liga sozinha e não se desliga sozinha é meia
    // funcionalidade: quem fechasse o portátil ficava com o telemóvel mudo e um
    // aviso a falar de um aparelho que já não está lá.
    expect(sessao).toMatch(/s\.on\('companion-ended', \(\) => \{\s*setCompanion\(false\)/)
    // O nome tem de ser o que o SERVIDOR envia: `ServerMsg` serializa em
    // kebab-case, e `companion_ended` nunca chegou a casar (encontrado na
    // integração do lote 1 — o modo companheiro nunca desligava no browser).
    const servidor = readFileSync(join(__dirname, '..', '..', 'server/src/signaling.rs'), 'utf8')
    expect(servidor).toMatch(/#\[serde\(tag = "type", rename_all = "kebab-case"\)\]\s*pub enum ServerMsg/)
    expect(servidor).toContain('CompanionEnded,')
    expect(rust).toMatch(/ServerMsg::CompanionEnded/)
    // E só quando resta UMA sessão: com três, sair uma deixa duas, e duas ainda
    // fazem eco.
    expect(rust).toMatch(/if restantes\.len\(\) == 1 \{/)
    expect(rust).toMatch(/com_tres_sessoes_sair_uma_nao_desliga_o_companion/)
  })

  it('voltar de um F5 não é um segundo dispositivo', () => {
    // O lugar reservado do R91 tem `disconnected_at`; trancar-lhe o áudio seria
    // castigar uma quebra de rede — o oposto do que o R91 foi resolver.
    expect(rust).toMatch(/reentrar_depois_de_uma_queda_nao_e_companion/)
  })
})
