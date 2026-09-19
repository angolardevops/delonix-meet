import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * A janela flutuante (PiP) é a única parte da sala cujo comportamento vive TODO
 * no browser: um `requestPictureInPicture` não corre em jsdom e não se finge.
 * Estes portões não provam que a janela abre — provam que as três armadilhas
 * que a fazem falhar em silêncio não voltam a entrar. O que eles NÃO cobrem
 * está escrito no PR e no catálogo, não escondido.
 */
const room = readFileSync(join(__dirname, 'pages', 'Room.tsx'), 'utf8')

describe('W3.5 · a janela flutuante não pode falhar em silêncio', () => {
  it('o <video> da janela não usa display:none', () => {
    // A armadilha: `display: none` parece a forma óbvia de esconder o elemento,
    // e é a única que o browser trata como «não tem imagem» — o pedido é
    // recusado sem erro visível. Esconde-se com 1×1 e opacidade zero.
    const i = room.indexOf('ref={pipVideo}')
    expect(i).toBeGreaterThan(0)
    const bloco = room.slice(i, i + 700)
    expect(bloco).not.toMatch(/display:\s*'none'/)
    expect(bloco).toMatch(/opacity:\s*0/)
  })

  it('o botão só aparece onde o browser suporta a janela', () => {
    // Firefox e o Safari de iOS não têm `pictureInPictureEnabled`. Um botão que
    // não faz nada é pior do que botão nenhum: a pessoa carrega, não acontece
    // nada, e conclui que o produto está partido.
    expect(room).toMatch(/document\.pictureInPictureEnabled === true/)
    expect(room).toMatch(/\{pipDisponivel && \(/)
  })

  it('a recusa do browser chega ao ecrã', () => {
    // Um `catch {}` vazio aqui era exactamente o R104 outra vez: o produto sabe
    // que falhou e a pessoa não. O `catch` do pedido escreve o aviso, e o aviso
    // está renderizado.
    const i = room.indexOf('await v.requestPictureInPicture()')
    expect(i).toBeGreaterThan(0)
    expect(room.slice(i, i + 400)).toMatch(/setPipErro\(t\('room\.pip\.recusada'\)\)/)
    expect(room).toMatch(/\{pipErro && \(/)
  })

  it('sala sem vídeo nenhum diz porquê em vez de abrir uma janela preta', () => {
    expect(room).toMatch(/setPipErro\(t\('room\.pip\.nadaParaMostrar'\)\)/)
  })

  it('a janela fechada pelo botão do browser apaga o estado', () => {
    // Sem este ouvinte, fechar pela janela do browser deixava o visto do menu
    // aceso e a fonte presa — e o carregar seguinte não reabria nada.
    expect(room).toMatch(/addEventListener\('leavepictureinpicture'/)
  })

  it('a decisão de quem aparece está fora do componente', () => {
    // A regra do repositório: decisão pura sai do React para poder ser testada
    // e MUTADA sem browser (como `layerPolicy.ts`). Se voltar para dentro do
    // `Room.tsx`, deixa de haver como prová-la.
    expect(room).toMatch(/from '\.\.\/pipPolicy'/)
    expect(room).not.toMatch(/function escolherFontePip/)
  })
})
