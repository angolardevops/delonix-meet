import { describe, expect, it } from 'vitest'
import en from '../locales/en/room'
import fr from '../locales/fr/room'
import pt from '../locales/pt/room'
import zh from '../locales/zh/room'

// As chaves que `DuplicateDeviceDialog.tsx` chama têm de existir nas quatro línguas (R99/R113):
// o teste de paridade só compara as línguas entre si, e passava com todas em falta.
const CHAVES = [
  'titulo', 'texto', 'soMeet', 'soMeetSemSom', 'soTelefone', 'nosDois', 'eco', 'ok', 'aTratar',
  'resultado.hung_up', 'resultado.muted', 'resultado.both', 'resultado.gone',
]

describe('dispositivos duplicados · textos nas quatro línguas', () => {
  for (const [lang, room] of Object.entries({ pt, en, fr, zh })) {
    it(`${lang}: todas as chaves de room.dispositivos existem e não estão vazias`, () => {
      const d = (room as unknown as { dispositivos?: Record<string, unknown> }).dispositivos
      expect(d, 'bloco room.dispositivos').toBeTruthy()
      for (const k of CHAVES) {
        const v = k.split('.').reduce<unknown>((o, p) => (o as Record<string, unknown> | undefined)?.[p], d)
        expect(typeof v === 'string' && v.length > 0, `${lang}: room.dispositivos.${k}`).toBe(true)
      }
    })
  }
})
