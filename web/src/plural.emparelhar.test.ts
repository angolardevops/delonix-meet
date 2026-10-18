import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import i18next from 'i18next'
import pt from './locales/pt/tv'
import en from './locales/en/tv'
import fr from './locales/fr/tv'
import zh from './locales/zh/tv'

/**
 * O #356 trouxe a contagem de fontes activas com o sufixo de plural do JSON
 * **v3** (`nActivos_plural`). Este repositório corre `i18next ^26` e o
 * `web/src/i18n.ts` NÃO passa `compatibilityJSON`, logo o formato é o v4,
 * cujo sufixo é `_other` — a chave `_plural` era simplesmente ignorada e o
 * ecrã do operador dizia «3 activo», «3 active», «3 actif». Em chinês
 * escapava por acidente, porque as duas formas são iguais.
 *
 * Isto mede com o i18next DO REPOSITÓRIO, não com a minha teoria sobre ele:
 * o portão de i18n não apanhava o defeito (a chave existia nas quatro
 * línguas) e nenhum teste renderizava a contagem.
 */
async function comDicionario(lng: string, tv: unknown) {
  const i = i18next.createInstance()
  await i.init({ lng, resources: { [lng]: { tv: tv as Record<string, unknown> } }, ns: ['tv'], defaultNS: 'tv' })
  return i
}

describe('a contagem de fontes activas concorda em número', () => {
  it('português: 1 activo, 3 activos', async () => {
    const i = await comDicionario('pt', pt)
    expect(i.t('emparelhar.nActivos', { count: 1 })).toBe('1 activo')
    expect(i.t('emparelhar.nActivos', { count: 3 })).toBe('3 activos')
  })

  it('inglês e francês também flectem', async () => {
    const ie = await comDicionario('en', en)
    expect(ie.t('emparelhar.nActivos', { count: 1 })).toBe('1 active')
    expect(ie.t('emparelhar.nActivos', { count: 3 })).toBe('3 active')
    const ifr = await comDicionario('fr', fr)
    expect(ifr.t('emparelhar.nActivos', { count: 1 })).toBe('1 actif')
    expect(ifr.t('emparelhar.nActivos', { count: 3 })).toBe('3 actifs')
  })

  it('chinês não flecte, e é isso que se espera', async () => {
    const i = await comDicionario('zh', zh)
    expect(i.t('emparelhar.nActivos', { count: 1 })).toBe('1 个有效')
    expect(i.t('emparelhar.nActivos', { count: 3 })).toBe('3 个有效')
  })

  it('CONTROLO NEGATIVO: o sufixo que lá estava produz mesmo a frase errada', async () => {
    // Sem isto, os testes acima passariam por acaso num dia em que o
    // i18next mudasse de regra. Isto fixa a RAZÃO: `_plural` é v3 e o v4
    // ignora-o, caindo no singular.
    const i = await comDicionario('pt', { emparelhar: { nActivos: '{{count}} activo', nActivos_plural: '{{count}} activos' } })
    expect(i.t('emparelhar.nActivos', { count: 3 })).toBe('3 activo')
  })

  it('nenhum dicionário volta a usar `_plural` — o sufixo v3 não resolve neste repositório', () => {
    for (const loc of ['pt', 'en', 'fr', 'zh']) {
      for (const f of ['tv', 'telemovel', 'studio', 'room']) {
        let src: string
        try {
          src = readFileSync(join(__dirname, 'locales', loc, `${f}.ts`), 'utf8')
        } catch {
          continue // um dicionário que não exista nesta língua não é defeito
        }
        expect(src, `${loc}/${f}.ts usa o sufixo v3 _plural`).not.toMatch(/\w+_plural\s*:/)
      }
    }
  })
})
