import { afterEach, describe, expect, it, vi } from 'vitest'
import { detectLanguage, intlLocale, LANG_NAMES, LANGS, resolveLang, serverLocale } from './i18n'

function fakeStorage(v: string | null) {
  vi.stubGlobal('localStorage', { getItem: () => v, setItem: () => {} })
}

describe('i18n · resolução de língua', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('pt, pt-PT e pt-BR resolvem para pt-AO; fr para fr-FR; qualquer zh para zh-CN', () => {
    for (const t of ['pt', 'pt-PT', 'pt-BR', 'pt_AO', 'PT-ao']) expect(resolveLang(t)).toBe('pt-AO')
    expect(resolveLang('en-US')).toBe('en')
    expect(resolveLang('fr')).toBe('fr-FR')
    expect(resolveLang('fr-CA')).toBe('fr-FR')
    for (const t of ['zh', 'zh-CN', 'zh-Hans-CN', 'zh-TW']) expect(resolveLang(t)).toBe('zh-CN')
    expect(resolveLang('de-DE')).toBeNull()
    expect(resolveLang('')).toBeNull()
    expect(resolveLang(null)).toBeNull()
  })

  it('a escolha guardada neste browser ganha a tudo — incluindo o valor antigo `fr`', () => {
    fakeStorage('fr')
    vi.stubGlobal('navigator', { languages: ['zh-CN'], language: 'zh-CN' })
    expect(detectLanguage('en')).toBe('fr-FR')
  })

  it('sem escolha local, a conta ganha ao browser — mas `pt` da conta é o valor por omissão e não conta', () => {
    fakeStorage(null)
    vi.stubGlobal('navigator', { languages: ['de-DE', 'zh-CN'], language: 'de-DE' })
    expect(detectLanguage('en')).toBe('en')
    expect(detectLanguage('pt')).toBe('zh-CN')
    expect(detectLanguage(undefined)).toBe('zh-CN')
  })

  it('sem nada que sirva, pt-AO — e um localStorage que rebenta não parte o arranque', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('bloqueado')
      },
    })
    vi.stubGlobal('navigator', { languages: ['de-DE'], language: 'de-DE' })
    expect(detectLanguage(null)).toBe('pt-AO')
  })

  it('Intl recebe um locale com região (o inglês em en-GB, 24 h)', () => {
    expect(intlLocale('pt-AO')).toBe('pt-AO')
    expect(intlLocale('en')).toBe('en-GB')
    expect(intlLocale('fr-FR')).toBe('fr-FR')
    expect(intlLocale('zh-CN')).toBe('zh-CN')
    expect(intlLocale('xx')).toBe('pt-AO')
  })

  it('cada língua tem nome escrito nela própria, e o servidor só recebe o que aceita', () => {
    expect(LANGS).toEqual(['pt-AO', 'en', 'fr-FR', 'zh-CN'])
    expect(LANG_NAMES['zh-CN']).toMatch(/[一-鿿]/)
    expect(LANGS.map(serverLocale)).toEqual(['pt', 'en', 'fr', null])
  })
})
