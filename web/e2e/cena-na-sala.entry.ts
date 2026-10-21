// MÓDULO DE APOIO do `cena-na-sala.mjs` — não é um teste.
//
// Expõe o compositor VERDADEIRO da sala (`room/compositor.ts`) ao script de
// Playwright. É a mesma classe que compõe o multicâmara que vai para RTMP;
// um arnês que reimplementasse a composição provaria o arnês.
import { RoomCompositor } from '../src/room/compositor'

declare global {
  interface Window {
    RoomCompositor: typeof RoomCompositor
  }
}

window.RoomCompositor = RoomCompositor
