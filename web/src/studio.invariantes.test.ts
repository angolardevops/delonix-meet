/**
 * Fitness functions do Estúdio.
 *
 * O comportamento é verificado por pixéis em `e2e/estudio.mjs`, num Chromium
 * com câmara e ecrã falsos. Estes testes guardam as decisões que um `git
 * revert` distraído desfaz sem nada ficar vermelho.
 */
import { readdirSync, readFileSync } from 'node:fs'
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
    // tamanhos diferentes. Na UI nova o seletor vive no seu componente
    // (`studio/RegionPicker.tsx`), e a página tem de o usar.
    expect(readCodigo('web/src/pages/Studio.tsx')).toContain("from '../studio/RegionPicker'")
    const s = readCodigo('web/src/studio/RegionPicker.tsx')
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
    // Os painéis `studio/*.tsx` da UI nova entram na mesma regra: um import
    // da sala escondido num sub-componente puxava o mesmo caminho de media.
    // Com o palco (layouts, cenas, mistura, legendas) a regra estende-se aos
    // módulos `.ts` do estúdio — um hook é tão capaz de arrastar o caminho de
    // media como um painel.
    const paineis = readdirSync(join(root, 'web/src/studio'))
      .filter((f) => /\.tsx?$/.test(f) && !f.endsWith('.test.ts'))
      .map((f) => `web/src/studio/${f}`)
    expect(paineis.length).toBeGreaterThan(0)
    for (const f of ['web/src/pages/Studio.tsx', 'web/src/studio/compositor.ts', ...paineis]) {
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

describe('a sala de convidados é OPCIONAL e carrega à parte', () => {
  // Receber convidados precisa de sinalização e SFU; gravar uma aula sozinho
  // não. A sala entra por `lazy()` num chunk próprio, e é o único sítio do
  // Estúdio onde `signaling`/`webrtc` aparecem — a regra acima continua a
  // valer para tudo o que o Estúdio carrega à partida.
  const pagina = () => readCodigo('web/src/pages/Studio.tsx')

  it('a página só a conhece por lazy(import())', () => {
    expect(pagina()).toContain("const SalaDoEstudio = lazy(() => import('./studio/SalaDoEstudio'))")
    expect(pagina()).not.toMatch(/^import [^\n]*SalaDoEstudio/m)
    expect(pagina()).not.toMatch(/^import [^\n]*useLigacaoDoEstudio/m)
  })

  it('e só a monta quando a pessoa pede', () => {
    expect(pagina()).toMatch(/\{salaAberta \? \(\s*<Suspense/)
  })

  it('o chunk da sala não casa com o padrão do precache do Estúdio', () => {
    // O precache escolhe `assets/Studio-*.js` e o seu FECHO ESTÁTICO. Um
    // `import()` dinâmico não entra nos `imports` do Rollup, e o nome do
    // chunk (`SalaDoEstudio-*`) não casa com o padrão.
    const cfg = readCodigo('web/vite.config.ts')
    const padrao = cfg.match(/const estudio = nomes\.filter\(\(f\) => (\/.*\/)\.test\(f\)\)/)?.[1]
    expect(padrao).toBeTruthy()
    const re = new RegExp(padrao!.slice(1, -1))
    expect(re.test('assets/Studio-abc123.js')).toBe(true)
    expect(re.test('assets/SalaDoEstudio-abc123.js')).toBe(false)
  })

  it('a queda da sala NÃO recarrega a página — levava a gravação e o directo', () => {
    // O `useCallSession` da sala recarrega quando o socket cai. No Estúdio,
    // isso destruía uma gravação a decorrer sem aviso.
    const l = readCodigo('web/src/pages/studio/useLigacaoDoEstudio.ts')
    expect(l).not.toContain('location.reload')
    expect(l).toContain("setEstado('caiu')")
  })

  it('a chamada só arranca depois de `joined` (R1/R2)', () => {
    const l = readCodigo('web/src/pages/studio/useLigacaoDoEstudio.ts')
    expect(l).toContain('makeCallHolderStart(')
    const joined = l.indexOf("sinal.on('joined'")
    expect(joined).toBeGreaterThan(-1)
    expect(l.slice(joined, joined + 400)).toContain('holder.start()')
  })
})

describe('a mistura tem faders de verdade', () => {
  const c = () => readCodigo('web/src/studio/compositor.ts')

  it('três GainNode de barramento, cada um ligado ao destino, mais um GainNode por convidado', () => {
    // 3 fixos (palco/música/vídeo, criados uma vez em montarFluxo) + 1 padrão
    // de código por convidado (criado sob procura em ligarConvidadoAoGrafo,
    // um por convidado em tempo de execução, mas um só sítio no ficheiro) —
    // o fader por convidado É um GainNode a mais, de propósito.
    expect(c().match(/this\.audioCtx\.createGain\(\)/g)?.length).toBe(4)
    expect(c()).toContain('for (const g of [this.ganhoPalco, this.ganhoMusica, this.ganhoVideo]) g.connect(this.destino)')
  })

  it('as fontes ligam-se ao GANHO, nunca direitas ao destino', () => {
    // Uma fonte ligada ao destino passava por cima do fader — que mexia sem
    // efeito nenhum. É o «campo que o sistema ignora» em forma de áudio.
    expect(c()).not.toMatch(/(micFonte|ecraFonte|musicaFonte|c\.audio)\.connect\(this\.destino\)/)
    expect(c()).toContain('this.micFonte.connect(this.ganhoPalco)')
    expect(c()).toContain('this.ecraFonte.connect(this.ganhoVideo)')
    expect(c()).toContain('this.musicaFonte.connect(this.ganhoMusica)')
    // O convidado tem fader PRÓPRIO agora: a fonte liga ao gain dele, e é
    // esse gain — não a fonte directamente — que liga ao barramento «Palco».
    expect(c()).toContain('c.audio.connect(c.gain)')
    expect(c()).toContain('c.gain.connect(this.ganhoPalco)')
  })

  it('a qualidade não muda com a gravação ou o directo a decorrer', () => {
    // Trocar o canvas por baixo de um fluxo capturado dava um ficheiro com
    // duas resoluções.
    expect(c()).toMatch(/definirQualidade\(p: PerfilDeQualidade\): boolean \{\s*if \(this\.consumidores > 0\) return false/)
  })

  it('o microfone escolhido chega à mistura', () => {
    expect(readCodigo('web/src/studio/usePalco.ts')).toContain('compRef.current?.trocarMicrofone(id)')
    expect(c()).toContain('deviceId: this.microfoneId ? { exact: this.microfoneId } : undefined')
  })
})

describe('as legendas no palco são locais', () => {
  it('o Transcriber arranca com preferLocal (sem Web Speech da Google)', () => {
    expect(readCodigo('web/src/studio/useLegendas.ts')).toMatch(/tr\.start\([^)]*, true\)/)
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
    expect(readCodigo('web/src/pages/Studio.tsx')).toContain('podeCortar={cortesSuportados()}')
    // «Com aviso» quer dizer um aviso NO ECRÃ, não só esconder os cursores.
    const edicao = readCodigo('web/src/studio/EditPanel.tsx')
    expect(edicao).toMatch(/!podeCortar \? \(\s*<Alert tone="warning">\{t\('studio\.edicao\.semWebCodecs'\)\}<\/Alert>/)
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
  // Os dicionários passaram a um ficheiro por área: o bloco `studio` é
  // `locales/<língua>/studio.ts`, composto no `index.ts`, e a entrada do rail
  // é `shell.nav.estudio`.
  for (const loc of ['pt', 'en', 'fr']) {
    it(loc, () => {
      const s = read(`web/src/locales/${loc}/studio.ts`)
      expect(s).toMatch(/^export default \{/)
      expect(read(`web/src/locales/${loc}/index.ts`)).toMatch(/import studio from '\.\/studio'/)
      expect(read(`web/src/locales/${loc}/shell.ts`)).toMatch(/^\s+estudio: '/m) // nav
      for (const k of ['gravar:', 'regiao:', 'posicao:', 'guardar:']) expect(s).toContain(k)
    })
  }
})
