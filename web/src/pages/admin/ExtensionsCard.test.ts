/**
 * O diálogo «Credenciais SIP» de um ramal desenhado para HTML. Não substitui um
 * browser (não prova layout: que nada se sobrepõe nem há scroll horizontal vê-se
 * num ecrã) — prova o que o diálogo DIZ: o servidor público quando existe, a
 * falta dele quando não existe, o número de acesso, um botão de copiar por
 * campo com nome, e nenhuma chave de tradução crua, nas quatro línguas.
 */
import { createElement as h } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import type { ExtensionCreated } from '../../api'

// O cliente da API lê a sessão do armazenamento ao carregar: em Node não existe.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })

const { default: i18n } = await import('../../i18n')
const { credentialFields, credentialsText, ExtensionCredentials } = await import('./ExtensionsCard')

// `setLanguage` mexe no `document`; aqui carregam-se os dicionários à mão.
const DICTS = {
  'pt-AO': null,
  en: () => import('../../locales/en'),
  'fr-FR': () => import('../../locales/fr'),
  'zh-CN': () => import('../../locales/zh'),
} as const
type Lng = keyof typeof DICTS
async function usar(lng: Lng) {
  const load = DICTS[lng]
  if (load && !i18n.hasResourceBundle(lng, 'translation')) {
    i18n.addResourceBundle(lng, 'translation', (await load()).default, true, true)
  }
  await i18n.changeLanguage(lng)
  expect(i18n.language).toBe(lng)
}

const semChavesCruas = (html: string) => expect(html).not.toMatch(/(consola|ui|org)\.[a-zA-Z]/)

const created = (over: Partial<ExtensionCreated> = {}): ExtensionCreated => ({
  id: 'e-1',
  org_id: 'org-1',
  member_id: 'u-1',
  member_username: 'ana',
  member_email: 'ana@exemplo.ao',
  extension: '101',
  sip_username: 'ramal_0123456789abcdef',
  label: '',
  active: true,
  created_at: '2026-10-03T10:00:00Z',
  meeting_access_number: '8000',
  sip_server: { host: 'meet.exemplo.ao', port: 5070, transport: 'udp', uri: 'sip:meet.exemplo.ao:5070;transport=udp' },
  sip_password: '0123456789abcdef0123456789abcd',
  sip_domain: 'uma-organizacao-de-nome-comprido.ramais.delonix.meet',
  ...over,
})

const render = (c: ExtensionCreated) => renderToStaticMarkup(h(ExtensionCredentials, { created: c }))
const rows = (html: string) => [...html.matchAll(/data-campo="([a-zA-Z]+)"/g)].map((m) => m[1])

describe('credenciais SIP de um ramal', () => {
  it('mostra o servidor público pronto a colar, antes do utilizador e da password', async () => {
    await usar('pt-AO')
    const html = render(created())
    expect(rows(html)).toEqual(['servidor', 'utilizador', 'password', 'dominio', 'numeroAcesso'])
    expect(html).toContain('sip:meet.exemplo.ao:5070;transport=udp')
    expect(html).toContain('ramal_0123456789abcdef')
    expect(html).toContain('0123456789abcdef0123456789abcd')
    expect(html).toContain('uma-organizacao-de-nome-comprido.ramais.delonix.meet')
    expect(html).toContain('8000')
    // O domínio lógico explica-se: não é o endereço a que o softphone se liga.
    expect(html).toContain('Não é o endereço a que o softphone se liga')
    semChavesCruas(html)
  })

  it('cada campo com valor tem o seu botão de copiar, com nome acessível', async () => {
    await usar('pt-AO')
    const html = render(created())
    const labels = [...html.matchAll(/<button[^>]*aria-label="([^"]+)"/g)].map((m) => m[1])
    expect(labels).toEqual([
      'Copiar: Servidor SIP (proxy)',
      'Copiar: Utilizador SIP',
      'Copiar: Password SIP',
      'Copiar: Domínio SIP',
      'Copiar: Número de acesso às reuniões',
    ])
  })

  it('sem endereço público, a linha do servidor diz que falta — não desaparece nem inventa', async () => {
    await usar('pt-AO')
    const html = render(created({ sip_server: null }))
    expect(rows(html)[0]).toBe('servidor')
    expect(html).toContain('não tem o endereço público do servidor SIP configurado')
    expect(html).not.toContain('sip:')
    // Não há o que copiar nessa linha: sobram os botões dos outros campos.
    expect([...html.matchAll(/<button/g)]).toHaveLength(4)
    semChavesCruas(html)
  })

  it('sem número de acesso, a linha não aparece', async () => {
    await usar('pt-AO')
    const html = render(created({ meeting_access_number: '' }))
    expect(rows(html)).toEqual(['servidor', 'utilizador', 'password', 'dominio'])
  })

  it('o «copiar tudo» inclui o servidor, e deixa-o de fora quando não existe', async () => {
    await usar('pt-AO')
    const t = i18n.t.bind(i18n)
    expect(credentialsText(credentialFields(created(), t))).toBe(
      [
        'Servidor SIP (proxy): sip:meet.exemplo.ao:5070;transport=udp',
        'Utilizador SIP: ramal_0123456789abcdef',
        'Password SIP: 0123456789abcdef0123456789abcd',
        'Domínio SIP: uma-organizacao-de-nome-comprido.ramais.delonix.meet',
        'Número de acesso às reuniões: 8000',
      ].join('\n'),
    )
    const semServidor = credentialsText(credentialFields(created({ sip_server: null }), t))
    expect(semServidor.split('\n')).toHaveLength(4)
    expect(semServidor).not.toContain('Servidor')
  })

  it('nenhuma chave crua em nenhuma das quatro línguas, com e sem servidor', async () => {
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      semChavesCruas(render(created()))
      semChavesCruas(render(created({ sip_server: null })))
      for (const key of ['consola.voz.ponte', 'consola.ramais.aviso', 'consola.ramais.acessoNota', 'consola.ramais.erroReservado']) {
        const text = i18n.t(key, { numero: '8000' })
        expect(text, `${lng} ${key}`).not.toBe(key)
        // Traduzido de facto, e não o português de recurso.
        if (lng !== 'pt-AO') expect(text, `${lng} ${key}`).not.toBe(i18n.t(key, { numero: '8000', lng: 'pt-AO' }))
      }
    }
    await usar('pt-AO')
  })

  it('os avisos dizem a condição, e não que a ponte não existe', async () => {
    await usar('en')
    expect(i18n.t('consola.voz.ponte')).not.toMatch(/does not exist/)
    expect(i18n.t('consola.voz.ponte')).toMatch(/only connects when this installation configures it/)
    expect(i18n.t('consola.ramais.aviso')).not.toMatch(/No extension joins/)
    expect(i18n.t('consola.ramais.aviso')).toMatch(/not yet been verified with a real call/)
    await usar('pt-AO')
  })
})
