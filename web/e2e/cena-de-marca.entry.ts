// MÓDULO DE APOIO do `cena-de-marca.mjs` — não é um teste.
//
// Expõe o compositor VERDADEIRO do Estúdio (`studio/compositor.ts`) ao script
// de Playwright. Nada aqui é uma cópia: a cena é desenhada pela mesma classe
// que grava e emite em produção, com os mesmos métodos de desenho de cada
// fonte. Um arnês que reimplementasse o desenho provaria o arnês.
import { CompositorDeAula } from '../src/studio/compositor'

declare global {
  interface Window {
    CompositorDeAula: typeof CompositorDeAula
  }
}

window.CompositorDeAula = CompositorDeAula
