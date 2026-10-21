/**
 * Portão da posição do menu de contexto.
 *
 * O caso que se vê sempre em produção e nunca em desenvolvimento: o botão
 * direito no ÚLTIMO retrato da grelha, ou na última linha da tabela, abre o
 * menu a dois pixéis da borda e metade dele fica fora do ecrã — sem barra de
 * scroll para o ir buscar, porque é `position: fixed`.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { posicaoDoMenu } from './Menu'

const ECRA = { largura: 1000, altura: 800 }
const MENU = { largura: 200, altura: 300 }

describe('posicaoDoMenu', () => {
  it('segue o ponteiro quando o menu cabe', () => {
    expect(posicaoDoMenu({ x: 100, y: 120 }, MENU, ECRA)).toEqual({ x: 100, y: 120 })
  })

  it('dobra para a esquerda e para cima junto às bordas', () => {
    expect(posicaoDoMenu({ x: 950, y: 780 }, MENU, ECRA)).toEqual({ x: 750, y: 480 })
  })

  it('dobra só no eixo que falta', () => {
    expect(posicaoDoMenu({ x: 950, y: 100 }, MENU, ECRA)).toEqual({ x: 750, y: 100 })
    expect(posicaoDoMenu({ x: 100, y: 780 }, MENU, ECRA)).toEqual({ x: 100, y: 480 })
  })

  it('encosta à margem quando nem dobrado cabe — um menu mais alto que o ecrã', () => {
    // Num telemóvel de 360×320 o menu de 300 de altura não cabe nem dobrado:
    // o `y` encosta (12 = 320 − 300 − 8) em vez de sair pela borda. No `x`
    // ainda há espaço para dobrar, e dobra (140 = 340 − 200).
    const estreito = { largura: 360, altura: 320 }
    const p = posicaoDoMenu({ x: 340, y: 300 }, MENU, estreito)
    expect(p).toEqual({ x: 140, y: 12 })
    expect(p.x).toBeGreaterThanOrEqual(8)
    expect(p.y).toBeGreaterThanOrEqual(8)
  })

  it('a margem é configurável e respeita-se nos dois eixos', () => {
    expect(posicaoDoMenu({ x: 990, y: 790 }, MENU, ECRA, 20)).toEqual({ x: 790, y: 490 })
  })
})

describe('o menu de contexto está ligado onde foi prometido', () => {
  const src = (p: string) => readFileSync(join(__dirname, '..', p), 'utf8')

  it('os retratos da sala, as duas vistas da biblioteca e a lista de contactos abrem-no', () => {
    // A app não tinha UM `onContextMenu`: o botão direito abria o menu do
    // browser por cima da reunião. Estes quatro são os sítios onde as acções
    // viviam escondidas num `hover` ou atrás de uma selecção.
    for (const f of [
      'room/ParticipantTile.tsx',
      'pages/recordings/RecordingGrid.tsx',
      'pages/recordings/RecordingTable.tsx',
      'pages/directory/ContactList.tsx',
    ]) {
      expect(src(f), f).toContain('onContextMenu=')
    }
  })

  it('o foco entra no menu DEPOIS de ele ter posição — nunca no efeito que o mede', () => {
    // Medido no browser, não deduzido: enquanto não há `pos` o menu está
    // `visibility: hidden` para não piscar no canto, e um `focus()` num
    // elemento invisível não faz nada. O menu abria com o foco no `body` e as
    // setas não andavam. A bateria não tem DOM — este portão lê a forma.
    const menu = src('ui/Menu.tsx')
    const efeitoDoFoco = /useEffect\(\(\) => \{\s*if \(!pos\) return\s*ref\.current\?\.querySelector<HTMLButtonElement>\('button:not\(:disabled\)'\)\?\.focus\(\)\s*\}, \[pos\]\)/
    expect(menu).toMatch(efeitoDoFoco)
    // E o efeito que MEDE não foca.
    const medir = menu.slice(menu.indexOf('useLayoutEffect'), menu.indexOf('/**\n   * O foco vai'))
    expect(medir).not.toContain('.focus()')
  })

  it('e o menu devolve o foco a quem o abriu, e fecha com Esc, com o scroll e com um clique fora', () => {
    const menu = src('ui/Menu.tsx')
    expect(menu).toContain('focoAnterior.current?.focus?.()')
    expect(menu).toContain("e.key === 'Escape'")
    expect(menu).toContain("window.addEventListener('scroll', onAnda, true)")
    expect(menu).toContain("document.addEventListener('pointerdown', onFora, true)")
  })
})
