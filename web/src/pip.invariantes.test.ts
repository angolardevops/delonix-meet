import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * A janela flutuante (PiP) é a única parte da sala cujo comportamento vive TODO
 * no browser: um `requestPictureInPicture` não corre em jsdom e não se finge.
 * Estes portões não provam que a janela abre — provam que as três armadilhas
 * que a fazem falhar em silêncio não voltam a entrar. O que eles NÃO cobrem
 * está escrito no PR e no catálogo, não escondido.
 *
 * Com a sala reconstruída, a lógica vive em `room/usePip.ts`, o `<video>`
 * escondido na página, e o botão e o aviso na barra de controlos.
 */
const ler = (...p: string[]) => readFileSync(join(__dirname, ...p), 'utf8')
const room = ler('pages', 'Room.tsx')
const pip = ler('room', 'usePip.ts')
const barra = ler('room', 'ControlBar.tsx')
const salaInteira = [room, ...readdirSync(join(__dirname, 'room')).filter((f) => /\.tsx?$/.test(f) && !f.includes('.test.')).map((f) => ler('room', f))].join('\n')

describe('W3.5 · a janela flutuante não pode falhar em silêncio', () => {
  it('o <video> da janela não usa display:none', () => {
    // A armadilha: `display: none` parece a forma óbvia de esconder o elemento,
    // e é a única que o browser trata como «não tem imagem» — o pedido é
    // recusado sem erro visível. Esconde-se com 1×1 e opacidade zero.
    const i = room.indexOf('ref={pip.pipVideo}')
    expect(i).toBeGreaterThan(0)
    const bloco = room.slice(i, i + 700)
    expect(bloco).not.toMatch(/display:\s*'none'/)
    expect(bloco).toMatch(/opacity:\s*0/)
    // E está montado em TODOS os estados da sala — antes de qualquer `if` de
    // estado —, senão o ouvinte de `leavepictureinpicture`, registado ao montar,
    // nunca o encontrava quando se entrava pela pré-entrada.
    expect(room.indexOf('ref={pip.pipVideo}')).toBeGreaterThan(room.indexOf('return (\n    <div className="dx-stage rm-room"'))
    expect(room.indexOf('ref={pip.pipVideo}')).toBeLessThan(room.indexOf('{content}'))
  })

  it('o botão só aparece onde o browser suporta a janela', () => {
    // Firefox e o Safari de iOS não têm `pictureInPictureEnabled`. Um botão que
    // não faz nada é pior do que botão nenhum: a pessoa carrega, não acontece
    // nada, e conclui que o produto está partido.
    expect(pip).toMatch(/document\.pictureInPictureEnabled === true/)
    expect(barra).toMatch(/\{pipDisponivel && \(/)
    expect(room).toMatch(/pipDisponivel=\{pip\.pipDisponivel\}/)
  })

  it('a recusa do browser chega ao ecrã', () => {
    // Um `catch {}` vazio aqui era exactamente o R104 outra vez: o produto sabe
    // que falhou e a pessoa não. O `catch` do pedido escreve o aviso, e o aviso
    // está renderizado — fora do menu, que fecha ao carregar.
    const i = pip.indexOf('await v.requestPictureInPicture()')
    expect(i).toBeGreaterThan(0)
    expect(pip.slice(i, i + 400)).toMatch(/setPipErro\(t\('room\.pip\.recusada'\)\)/)
    expect(barra).toMatch(/\{pipErro && \(/)
    expect(room).toMatch(/pipErro=\{pip\.pipErro\}/)
  })

  it('sala sem vídeo nenhum diz porquê em vez de abrir uma janela preta', () => {
    expect(pip).toMatch(/setPipErro\(t\('room\.pip\.nadaParaMostrar'\)\)/)
  })

  it('a janela fechada pelo botão do browser apaga o estado', () => {
    // Sem este ouvinte, fechar pela janela do browser deixava o visto do menu
    // aceso e a fonte presa — e o carregar seguinte não reabria nada.
    expect(pip).toMatch(/addEventListener\('leavepictureinpicture'/)
  })

  it('a decisão de quem aparece está fora do componente', () => {
    // A regra do repositório: decisão pura sai do React para poder ser testada
    // e MUTADA sem browser (como `layerPolicy.ts`). Se voltar para dentro da
    // sala, deixa de haver como prová-la.
    expect(pip).toMatch(/from '\.\.\/pipPolicy'/)
    expect(salaInteira).not.toMatch(/function escolherFontePip/)
  })
})
