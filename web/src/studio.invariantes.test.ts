/**
 * Fitness functions do Estúdio.
 *
 * O comportamento é verificado por pixéis em `e2e/estudio.mjs`, num Chromium
 * com câmara e ecrã falsos. Estes testes guardam as decisões que um `git
 * revert` distraído desfaz sem nada ficar vermelho.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { AVATAR_INICIAL, ECRA_PARA_GRAVACAO, RECORTE_INTEIRO } from './studio/compositor'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')

/**
 * Lê o ficheiro SEM comentários.
 *
 * Existe porque o portão da fonte silenciosa não ficou vermelho quando a linha
 * foi comentada: o `toContain` encontrava a string dentro do comentário e dava
 * verde a código morto. Um teste que aceita a linha comentada não guarda nada.
 */
const readCodigo = (p: string) =>
  read(p)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .filter((l) => !l.trimStart().startsWith('//'))
    .join('\n')

describe('o ecrã é capturado para GRAVAR, não para partilhar', () => {
  it('pede 30 fps, não os 5 da partilha em chamada', () => {
    const v = ECRA_PARA_GRAVACAO.video as MediaTrackConstraints
    expect((v.frameRate as ConstrainULongRange).ideal).toBe(30)
  })

  it('e não herda as SCREEN_CONSTRAINTS do webrtc', () => {
    // Aquelas pedem `frameRate: { ideal: 5, max: 15 }` — certo para poupar
    // banda numa chamada, aos solavancos numa aula gravada.
    const webrtc = read('web/src/webrtc.ts')
    expect(webrtc).toContain('frameRate: { ideal: 5, max: 15 }')
    expect(read('web/src/studio/compositor.ts')).not.toContain("from './webrtc'")
  })

  it('pede 1080p', () => {
    const v = ECRA_PARA_GRAVACAO.video as MediaTrackConstraints
    expect((v.height as ConstrainULongRange).ideal).toBe(1080)
  })
})

describe('o recorte é em fracções, não em pixéis', () => {
  it('o rectângulo inteiro é 0,0 → 1,1', () => {
    expect(RECORTE_INTEIRO).toEqual({ x: 0, y: 0, w: 1, h: 1 })
  })

  it('o seletor guarda fracções', () => {
    // Guardar pixéis da pré-visualização partia o recorte assim que a janela
    // mudasse de tamanho — a pré-visualização e o canvas de gravação têm
    // tamanhos diferentes.
    const s = readCodigo('web/src/pages/Studio.tsx')
    expect(s).toContain('(e.clientX - r.left) / r.width')
    expect(s).toContain('(e.clientY - r.top) / r.height')
  })
})

describe('o avatar', () => {
  it('arranca visível, no canto inferior-direito', () => {
    expect(AVATAR_INICIAL.visivel).toBe(true)
    expect(AVATAR_INICIAL.canto).toBe('inferior-direito')
  })

  it('o compositor sabe desenhar os quatro cantos', () => {
    const c = readCodigo('web/src/studio/compositor.ts')
    for (const canto of ['inferior-direito', 'inferior-esquerdo', 'superior-direito', 'superior-esquerdo']) {
      expect(c).toContain(`case '${canto}'`)
    }
  })

  it('a bolha nunca sai do enquadramento', () => {
    const c = readCodigo('web/src/studio/compositor.ts')
    expect(c).toContain('cx = Math.min(W - lado / 2, Math.max(lado / 2, cx))')
    expect(c).toContain('cy = Math.min(H - lado / 2, Math.max(lado / 2, cy))')
  })
})

describe('a gravação não sai vazia', () => {
  it('há uma fonte de áudio silenciosa sempre ligada', () => {
    // Um destino de áudio sem entradas não produz amostras e o muxer do
    // MediaRecorder bloqueia — a gravação sai vazia quando não há microfone.
    // É a mesma armadilha que o MeetingRecorder já documentava.
    const c = readCodigo('web/src/studio/compositor.ts')
    expect(c).toContain('createConstantSource()')
    expect(c).toContain('silencio.connect(this.destino)')
  })

  it('e o utilizador é avisado se ainda assim sair vazia', () => {
    expect(read('web/src/pages/Studio.tsx')).toContain("t('studio.vazia'")
  })
})

describe('o estúdio não depende do caminho de media da sala', () => {
  it('não importa webrtc, signaling nem e2ee', () => {
    for (const f of ['web/src/pages/Studio.tsx', 'web/src/studio/compositor.ts']) {
      const s = readCodigo(f)
      for (const mod of ['webrtc', 'signaling', 'e2ee']) {
        expect(s).not.toMatch(new RegExp(`from '.*/${mod}'`))
      }
    }
  })

  it('a página entra por lazy, como as outras pesadas', () => {
    const app = readCodigo('web/src/App.tsx')
    expect(app).toContain("const Studio = lazy(() => import('./pages/Studio'))")
    expect(app).not.toMatch(/^import Studio from/m)
  })
})

describe('o recorte de fundo reutiliza o que já existe', () => {
  it('a pessoa com alfa vem do BackgroundEffect, não de um segmentador novo', () => {
    // O motor (RVM + MediaPipe, com a borda suavizada) já estava no repo. O
    // que faltava era ACESSO: a track que o `start()` devolve é opaca, e para
    // sobrepor a pessoa aos slides é preciso o canvas com alfa.
    expect(readCodigo('web/src/media.ts')).toContain('get pessoaComAlfa()')
    expect(readCodigo('web/src/pages/Studio.tsx')).toContain("new BackgroundEffect()")
    expect(readCodigo('web/src/studio/compositor.ts')).not.toContain('tasks-vision')
  })

  it('sem o primeiro frame da segmentação, cai na bolha em vez de um buraco', () => {
    const c = readCodigo('web/src/studio/compositor.ts')
    expect(c).toContain("if (this.avatar.modo === 'recorte' && this.pessoaComAlfa)")
  })
})

describe('a bolha arrasta-se', () => {
  it('o arrasto guarda fracções e passa o canto a `livre`', () => {
    const s = readCodigo('web/src/pages/Studio.tsx')
    expect(s).toContain("canto: 'livre'")
    expect(s).toContain('(ev.clientX - r.left) / r.width')
  })

  it('o compositor sabe desenhar em posição livre', () => {
    expect(readCodigo('web/src/studio/compositor.ts')).toContain('cx = this.avatar.x * W')
  })
})

describe('o corte é de pouco recurso', () => {
  it('não traz ffmpeg.wasm', () => {
    // ~30 MB de WASM a descarregar e a compilar, a correr em software num
    // fio só. O pedido era «cortes profissionais com pouco recurso».
    const pkg = read('web/package.json')
    expect(pkg).not.toContain('ffmpeg')
    expect(pkg).toContain('webm-muxer')
  })

  it('usa WebCodecs — decodificação por hardware', () => {
    const e = readCodigo('web/src/studio/editor.ts')
    expect(e).toContain('new VideoEncoder(')
    expect(e).toContain('MediaStreamTrackProcessor')
  })

  it('degrada com aviso onde o WebCodecs não existe', () => {
    expect(readCodigo('web/src/studio/editor.ts')).toContain('export function cortesSuportados()')
    expect(readCodigo('web/src/pages/Studio.tsx')).toContain('cortesSuportados()')
  })
})

describe('o áudio do corte não sai em falsete', () => {
  it('não é capturado do <video> acelerado', () => {
    // Acelerar a reprodução para cortar depressa comprime o áudio no tempo e
    // sobe-lhe o tom. Um corte com a voz do professor em falsete não é um
    // corte. O áudio vem da faixa ISOLADA e é fatiado por amostras.
    // `toContain` não serve para nomes de função: `decodeAudioDataX` contém
    // `decodeAudioData` e o portão passava com a chamada trocada. Fronteira
    // de palavra, sempre.
    const e = readCodigo('web/src/studio/editor.ts')
    expect(e).toMatch(/\bdecodeAudioData\(/)
    expect(e).toMatch(/\bcopyToChannel\(/)
    expect(e).toMatch(/\bnew AudioEncoder\(/)
  })

  it('o corte de áudio é por índice de amostra, não por tempo aproximado', () => {
    // O `fatiarAudio` passou a receber VÁRIOS troços (remoção de pausas), por
    // isso a variável mudou de `troco` para `t` — mas o invariante é o mesmo:
    // as fronteiras são índices de amostra, exactos, não tempos arredondados
    // a limites de pacote.
    const e = readCodigo('web/src/studio/editor.ts')
    expect(e).toContain('Math.floor(t.inicio * sr)')
    expect(e).toContain('Math.ceil(t.fim * sr)')
  })

  it('os troços de áudio são copiados SEGUIDOS, para acompanhar a imagem', () => {
    // Ao remover pausas do meio, o áudio tem de fechar os buracos na mesma
    // ordem que a imagem. Copiar cada janela para o seu tempo ORIGINAL deixaria
    // o som a arrastar-se atrás da imagem, cada vez mais desfasado.
    const e = readCodigo('web/src/studio/editor.ts')
    expect(e).toContain('fatia.copyToChannel(origem.subarray(j.de, j.ate), c, escrito)')
    expect(e).toContain('escrito += j.ate - j.de')
  })
})

describe('as faixas são separadas POR DESENHO', () => {
  it('há um gravador por faixa, além do combinado', () => {
    const c = readCodigo('web/src/studio/compositor.ts')
    expect(c).toContain('private gravadorVideo: MediaRecorder | null')
    expect(c).toContain('private gravadorAudio: MediaRecorder | null')
  })

  it('as faixas isoladas reusam as MESMAS tracks — sem segunda composição', () => {
    const c = readCodigo('web/src/studio/compositor.ts')
    expect(c).toContain('new MediaStream(stream.getVideoTracks())')
    expect(c).toContain('new MediaStream(faixasAudio)')
  })

  it('pausar e retomar abrangem os três gravadores', () => {
    // Um gravador esquecido na pausa desalinha as faixas e o «juntar» sai
    // dessincronizado — que é o defeito que ninguém repara até ao fim.
    // Conta-se: `pausar`, `retomar` e `destruir` têm de iterar os três. Um
    // `toContain` simples passava com dois deles partidos.
    const c = readCodigo('web/src/studio/compositor.ts')
    const iteracoes = (c.match(/for \(const g of this\.todos\)/g) ?? []).length
    expect(iteracoes).toBeGreaterThanOrEqual(3)
  })
})

describe('o codec do multiplexador segue o do encoder', () => {
  const e = () => readCodigo('web/src/studio/editor.ts')

  it('o perfil é escolhido ANTES do multiplexador', () => {
    // Se o multiplexador for construído primeiro, alguém acaba por fixar o
    // codec nele — e um ficheiro rotulado com o codec errado abre sem duração
    // e sem imagem, sem erro em lado nenhum.
    const s = e()
    expect(s.indexOf('const perfil = await escolherPerfil(')).toBeLessThan(s.indexOf('new Muxer({'))
  })

  it('o codec Matroska é DERIVADO do perfil, não escrito à mão', () => {
    expect(e()).toContain("perfil.codec.startsWith('vp09') ? 'V_VP9' : 'V_VP8'")
    expect(e()).not.toMatch(/video: \{ codec: 'V_VP[89]'/)
  })

  it('e o encoder recebe o MESMO perfil', () => {
    expect(e()).toContain('encoder.configure(perfil)')
  })
})

describe('o corte degrada em vez de arrastar', () => {
  const e = () => readCodigo('web/src/studio/editor.ts')

  it('pergunta ao browser em vez de assumir hardware', () => {
    // `configure()` NÃO falha sem hardware: cai para software em silêncio, e um
    // VP9 de 1080p em software leva minutos onde levava segundos. O sintoma é
    // «a barra não anda», que ninguém liga à falta de GPU.
    expect(e()).toContain('VideoEncoder.isConfigSupported(')
    expect(e()).toContain("hardwareAcceleration: 'prefer-hardware'")
  })

  it('tem um último recurso que o software aguenta', () => {
    // Sem este degrau, uma máquina sem aceleração fica sem corte nenhum.
    //
    // Conta DUAS ocorrências, e é de propósito: o perfil aparece na lista de
    // candidatos E como `return` final. Uma asserção de presença casava com o
    // `return` e dava verde com o candidato apagado — foi o que a primeira
    // versão deste teste fazia (medido: 2 ocorrências com, 1 sem).
    const n = e().match(/codec: 'vp8', bitrate: 2_000_000/g)?.length ?? 0
    expect(n).toBe(2)
  })
})

describe('as três línguas têm as chaves do estúdio', () => {
  for (const loc of ['pt', 'en', 'fr']) {
    it(loc, () => {
      const s = read(`web/src/locales/${loc}.ts`)
      expect(s).toContain('  studio: {')
      expect(s).toMatch(/^\s+studio: '/m) // nav.studio
      for (const k of ['gravar:', 'regiao:', 'posicao:', 'guardar:']) expect(s).toContain(k)
    })
  }
})
