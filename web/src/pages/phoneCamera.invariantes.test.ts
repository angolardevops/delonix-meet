import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const PC = readFileSync(join(__dirname, 'PhoneCamera.tsx'), 'utf8')
const LOCAL_MEDIA = readFileSync(join(__dirname, '..', 'room', 'useLocalMedia.ts'), 'utf8')

/**
 * As três regressões com que o #356 foi fundido, e que esta bateria trava.
 * São invariantes de FONTE e não de comportamento: este repositório não tem
 * arnês para montar componentes React, e o que aqui se mede — a ordem de duas
 * chamadas, a lista de dependências de um efeito, a existência de um
 * `onerror` — é exactamente o que se perde num refactor distraído.
 *
 * O que NÃO se prova aqui, e eu digo-o em vez de deixar supor: ninguém abriu
 * esta página num telefone. A prova de que o ecrã fica aceso, de que o
 * ficheiro de segurança abre, e de que o cursor de zoom serve a lente nova
 * tem de ser feita por uma pessoa com um aparelho na mão.
 */
describe('o telefone aguenta ser câmara', () => {
  it('virar a lente FECHA a gravação antes de trocar a pista, e reabre depois', () => {
    // Porque é obrigatório: o `switchCam` faz removeTrack/addTrack no MESMO
    // MediaStream sobre que o MediaRecorder foi construído, e pela
    // especificação isso termina a gravação com InvalidModificationError.
    expect(LOCAL_MEDIA).toMatch(/removeTrack\(old\)/)
    expect(LOCAL_MEDIA).toMatch(/addTrack\(newTrack\)/)

    const virar = PC.slice(PC.indexOf('const virarLente'), PC.indexOf('useEffect(() => {\n    const guarda'))
    expect(virar).toContain('pararGravacaoSeguranca()')
    expect(virar).toContain('iniciarGravacaoSeguranca()')

    // A ORDEM é a correcção: parar ANTES do flipCam, arrancar DEPOIS. Ao
    // contrário, o ficheiro saía truncado no instante da viragem.
    const parar = virar.indexOf('pararGravacaoSeguranca()')
    const flip = virar.indexOf('media.flipCam()')
    const arrancar = virar.indexOf('iniciarGravacaoSeguranca()')
    expect(parar).toBeGreaterThan(-1)
    expect(parar).toBeLessThan(flip)
    expect(flip).toBeLessThan(arrancar)
  })

  it('um pedaço fechado ACRESCENTA-SE à lista em vez de substituir o anterior', () => {
    // Com um campo só, virar a lente fechava o pedaço 1, abria o 2, e o 1
    // desaparecia do ecrã — perdia-se o que já estava gravado.
    expect(PC).toMatch(/setFicheiros\(\(fs\) => \[\.\.\.fs,/)
    expect(PC).not.toMatch(/setFicheiroSeguranca/)
  })

  it('uma gravação que morre sozinha DIZ-O, em vez de deixar o botão vermelho a mentir', () => {
    expect(PC).toMatch(/rec\.onerror\s*=/)
    const onerror = PC.slice(PC.indexOf('rec.onerror'), PC.indexOf('rec.start('))
    expect(onerror).toContain('setGravando(false)')
    expect(onerror).toContain("t('telemovel.segurancaMorreu')")
  })

  it('as capacidades da câmara são relidas na lente NOVA', () => {
    // A regressão: as dependências eram `[core.cameraTrackRef]`, um objecto
    // ref de identidade estável — o efeito corria uma vez na montagem e os
    // limites de zoom/foco ficavam os da primeira lente para sempre.
    const efeito = PC.slice(PC.indexOf('const caps = (track?.getCapabilities'))
    const deps = efeito.slice(efeito.indexOf('}, ['), efeito.indexOf('])') + 2)
    expect(deps).toContain('media.facing')
    expect(deps).toContain('media.camId')
    // E o `facing`/`camId` têm de ser mesmo escritos pelo switchCam, senão a
    // dependência nunca muda e isto volta a correr uma vez só.
    expect(LOCAL_MEDIA).toMatch(/setFacing\(facingOf\(newTrack\)\)/)
    expect(LOCAL_MEDIA).toMatch(/setCamId\(/)
  })

  it('o ecrã pede o lock e volta a pedi-lo — a lógica e as provas vivem em room/wakeLock.ts', () => {
    expect(PC).toContain('criarGuardaDeEcra')
    expect(PC).toMatch(/addEventListener\(\s*['"]visibilitychange['"]/)
    // E larga-o ao sair: um lock que sobrevive à página deixa o ecrã do
    // telefone aceso para sempre depois de fechar a câmara.
    expect(PC).toMatch(/guarda\.largar\(\)/)
  })
})
