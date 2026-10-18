// MÓDULO DE APOIO do `cena-de-marca.mjs` — não é um teste.
//
// Expõe o compositor VERDADEIRO do Estúdio (`studio/compositor.ts`) ao script
// de Playwright. Nada aqui é uma cópia: a cena é desenhada pela mesma classe
// que grava e emite em produção, com os mesmos métodos de desenho de cada
// fonte. Um arnês que reimplementasse o desenho provaria o arnês.
import { CompositorDeAula } from '../src/studio/compositor'
// A GEOMETRIA vai também, e pela mesma razão: as zonas de amostragem da coluna
// saem do `rectsDaMoldura` REAL, não de fracções escritas à mão no script. A
// lição da sessão anterior foi que uma zona com um píxel a mais mede a moldura
// preta em vez da fonte, e dá um número redondo que parece um resultado. Com a
// zona derivada da mesma função que desenha, ela não pode divergir do desenho —
// e a geometria em si já tem 26 testes puros em `cena/moldura.test.ts`, que é
// onde ela se prova.
import { rectsDaMoldura } from '../src/cena/moldura'
// O `rectsDoLayout` vai pela MESMA razão, e serve as passagens que não são da
// `moldura`: a regra do `programaId` aplica-se a mais layouts do que um, e um
// teste que só sabe medir a moldura não consegue dizer onde a regra começa e
// acaba. Era o buraco que deixava passar «reordenar só na moldura».
import { rectsDoLayout } from '../src/studio/palco'

declare global {
  interface Window {
    CompositorDeAula: typeof CompositorDeAula
    rectsDaMoldura: typeof rectsDaMoldura
    rectsDoLayout: typeof rectsDoLayout
  }
}

window.CompositorDeAula = CompositorDeAula
window.rectsDaMoldura = rectsDaMoldura
window.rectsDoLayout = rectsDoLayout
