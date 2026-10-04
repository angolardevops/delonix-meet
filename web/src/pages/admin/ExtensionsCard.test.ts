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
import type { Extension, ExtensionCreated } from '../../api'

// O cliente da API lê a sessão do armazenamento ao carregar: em Node não existe.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })

const { default: i18n } = await import('../../i18n')
const { ApiError } = await import('../../api')
const { assignOutcomeText, credentialFields, credentialsText, ExtensionCredentials, ExtensionList, pinErrorMessage } =
  await import('./ExtensionsCard')
const { MyExtensionList, myPinErrorMessage } = await import('../../components/MyExtensionPanel')

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

const semChavesCruas = (html: string) => expect(html).not.toMatch(/(consola|ui|org|shell)\.[a-zA-Z]/)

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
  pin_state: 'unset',
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

// ---------- R276: PIN do ramal, ramais da empresa, atribuição em massa ----------

const ramal = (over: Partial<Extension> = {}): Extension => ({
  id: 'e-1',
  org_id: 'org-1',
  member_id: 'u-1',
  member_username: 'ana',
  member_email: 'ana@exemplo.ao',
  extension: '1004',
  sip_username: 'ramal_0123456789abcdef',
  label: '',
  active: true,
  created_at: '2026-10-04T10:00:00Z',
  pin_state: 'unset',
  meeting_access_number: '8000',
  sip_server: null,
  ...over,
})

const nada = () => {}
const lista = (list: Extension[]) =>
  renderToStaticMarkup(
    h(ExtensionList, {
      list,
      busyId: null,
      onToggleActive: nada,
      onRegeneratePassword: nada,
      onRemove: nada,
      onGeneratePin: nada,
      onChoosePin: nada,
      onClearPin: nada,
    }),
  )
/** O HTML de UMA linha, pelo número do ramal. */
const linha = (html: string, numero: string) => {
  const m = html.match(new RegExp(`<li[^>]*data-ramal="${numero}"[\\s\\S]*?</li>`))
  expect(m, `linha do ramal ${numero}`).not.toBeNull()
  return m![0]
}
const botoes = (html: string) => [...html.matchAll(/<button[^>]*>(?:<[^>]+>|\s)*([^<]+)</g)].map((m) => m[1].trim())

const QUATRO: Extension[] = [
  ramal(),
  ramal({ id: 'e-2', member_id: 'u-2', member_username: 'rui', member_email: 'rui@exemplo.ao', extension: '1005', pin_state: 'set', label: 'Suporte' }),
  ramal({ id: 'e-3', member_id: 'u-3', member_username: 'eva', member_email: 'eva@exemplo.ao', extension: '1006', pin_state: 'locked' }),
  ramal({ id: 'e-4', member_id: null, member_username: null, member_email: null, extension: '2000', label: 'Recepção' }),
]

describe('lista dos ramais (R276)', () => {
  it('mostra o ESTADO do PIN por linha — definido, por definir, bloqueado', async () => {
    await usar('pt-AO')
    const html = lista(QUATRO)
    expect([...html.matchAll(/data-pin-state="([a-z]+)"/g)].map((m) => m[1])).toEqual(['unset', 'set', 'locked', 'unset'])
    expect(linha(html, '1004')).toContain('PIN por definir')
    expect(linha(html, '1005')).toContain('PIN definido')
    expect(linha(html, '1006')).toContain('PIN bloqueado')
    semChavesCruas(html)
  })

  it('não é uma tabela: cada linha quebra em vez de rolar na horizontal', async () => {
    await usar('pt-AO')
    const html = lista(QUATRO)
    expect(html).not.toMatch(/<table|<td|dx-table-wrap/)
    expect(html).toContain('data-testid="ramais-list"')
    expect([...html.matchAll(/class="org-ext__actions"/g)]).toHaveLength(4)
  })

  it('o PIN de uma pessoa é dela: quem administra só o pode apagar, e só se existir', async () => {
    await usar('pt-AO')
    const html = lista(QUATRO)
    // Por definir: não há nada para apagar, e nunca se gera nem se escolhe por ela.
    expect(botoes(linha(html, '1004'))).toEqual(['Desactivar', 'Regenerar password', 'Apagar'])
    expect(botoes(linha(html, '1005'))).toEqual(['Desactivar', 'Regenerar password', 'Forçar PIN novo', 'Apagar'])
    expect(botoes(linha(html, '1006'))).toEqual(['Desactivar', 'Regenerar password', 'Forçar PIN novo', 'Apagar'])
  })

  it('um ramal da empresa mostra a etiqueta, e o PIN é de quem administra', async () => {
    await usar('pt-AO')
    const html = lista([...QUATRO, ramal({ id: 'e-5', member_id: null, member_username: null, member_email: null, extension: '2001', label: 'Portaria', pin_state: 'set' })])
    const recepcao = linha(html, '2000')
    expect(recepcao).toContain('data-tipo="empresa"')
    expect(recepcao).toContain('<strong>Recepção</strong>')
    expect(recepcao).toContain('Ramal da empresa')
    expect(botoes(recepcao)).toEqual(['Desactivar', 'Regenerar password', 'Gerar PIN', 'Escolher PIN', 'Apagar'])
    expect(botoes(linha(html, '2001'))).toEqual(['Desactivar', 'Regenerar password', 'Gerar PIN', 'Escolher PIN', 'Limpar PIN', 'Apagar'])
    expect(linha(html, '1005')).toContain('data-tipo="pessoa"')
    // A etiqueta de um ramal de pessoa fica ao lado do email, não no lugar do nome.
    expect(linha(html, '1005')).toContain('rui@exemplo.ao · Suporte')
  })

  it('as acções de cada linha são um grupo com o número do ramal no nome', async () => {
    await usar('pt-AO')
    expect(lista(QUATRO)).toContain('role="group" aria-label="Acções do ramal 2000"')
  })

  it('nenhuma chave crua, e tudo traduzido, nas quatro línguas', async () => {
    const keys = [
      'consola.ramais.daEmpresa',
      'consola.ramais.empresaDica',
      'consola.ramais.erroEtiqueta',
      'consola.ramais.tipoPessoa',
      'consola.ramais.pin.nota',
      'consola.ramais.pin.estado.unset',
      'consola.ramais.pin.estado.set',
      'consola.ramais.pin.estado.locked',
      'consola.ramais.pin.forcarConfirmar',
      'consola.ramais.pin.limparConfirmar',
      'consola.ramais.pin.reveladoTitulo',
      'consola.ramais.pin.reveladoAviso',
      'consola.ramais.pin.regras',
      'consola.ramais.pin.erro.formato',
      'consola.ramais.pin.erro.repetido',
      'consola.ramais.pin.erro.sequencia',
      'consola.ramais.pin.erro.contemRamal',
      'consola.ramais.pin.erro.dePessoa',
      'consola.ramais.pin.erro.generico',
      'consola.ramais.atribuir.titulo',
      'consola.ramais.atribuir.dica',
      'consola.ramais.atribuir.botao',
      'consola.ramais.atribuir.nada',
      'consola.ramais.atribuir.notaCredenciais',
      'consola.ramais.atribuir.erroIntervalo',
      'shell.def.ramal.titulo',
      'shell.def.ramal.nota',
      'shell.def.ramal.semRamal',
      'shell.def.ramal.estado.locked',
      'shell.def.ramal.gerar',
      'shell.def.ramal.escolher',
      'shell.def.ramal.revelado',
      'shell.def.ramal.erro.contemRamal',
    ]
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      semChavesCruas(lista(QUATRO))
      for (const key of keys) {
        const text = i18n.t(key, { numero: '8000', extensao: '1004' })
        expect(text, `${lng} ${key}`).not.toBe(key)
        if (lng !== 'pt-AO') expect(text, `${lng} ${key}`).not.toBe(i18n.t(key, { numero: '8000', extensao: '1004', lng: 'pt-AO' }))
      }
      for (const count of [1, 3]) {
        for (const key of ['consola.ramais.atribuir.feito', 'consola.ramais.atribuir.esgotado']) {
          const text = i18n.t(key, { count })
          expect(text, `${lng} ${key} ${count}`).toContain(String(count))
          expect(text, `${lng} ${key} ${count}`).not.toContain(key)
        }
      }
    }
    await usar('pt-AO')
  })

  it('o texto não promete o que não existe: ainda nenhuma chamada pede o PIN', async () => {
    await usar('pt-AO')
    expect(i18n.t('consola.ramais.pin.nota')).toMatch(/Ainda nenhuma chamada o pede/)
    expect(i18n.t('shell.def.ramal.nota')).toMatch(/Ainda nenhuma chamada o pede/)
    await usar('en')
    expect(i18n.t('consola.ramais.pin.nota')).toMatch(/No call asks for it yet/)
    await usar('pt-AO')
  })
})

describe('atribuir ramais a todos (R276)', () => {
  it('diz o que foi criado, o que ficou de fora e quando não havia nada a fazer', async () => {
    await usar('pt-AO')
    const t = i18n.t.bind(i18n)
    expect(assignOutcomeText({ created: 3, remaining: 0, exhausted: false }, t)).toBe('Foram criados 3 ramais.')
    expect(assignOutcomeText({ created: 1, remaining: 0, exhausted: false }, t)).toBe('Foi criado 1 ramal.')
    expect(assignOutcomeText({ created: 0, remaining: 0, exhausted: false }, t)).toBe('Todas as pessoas activas já têm ramal.')
    expect(assignOutcomeText({ created: 2, remaining: 5, exhausted: true }, t)).toBe(
      'Foram criados 2 ramais. O intervalo esgotou-se: 5 pessoas ficaram sem ramal. Alarga o intervalo e repete.',
    )
    // Intervalo esgotado sem criar nada: não diz que todos têm ramal.
    expect(assignOutcomeText({ created: 0, remaining: 1, exhausted: true }, t)).toBe(
      'O intervalo esgotou-se: 1 pessoa ficou sem ramal. Alarga o intervalo e repete.',
    )
  })
})

describe('recusas do PIN (R276)', () => {
  const recusa = (code: string, status = 400) => new ApiError(status, { error: 'x', code }, 'x')

  it('cada código do servidor tem a sua frase, na consola e na área da pessoa', async () => {
    await usar('pt-AO')
    const t = i18n.t.bind(i18n)
    for (const [code, frase] of [
      ['ramais.pin_format', 'exactamente 6 dígitos'],
      ['ramais.pin_repeated', 'todos iguais'],
      ['ramais.pin_sequence', 'sequência'],
      ['ramais.pin_contains_extension', 'número do ramal'],
    ] as const) {
      expect(pinErrorMessage(recusa(code), t)).toContain(frase)
      expect(myPinErrorMessage(recusa(code), t)).toContain(frase)
    }
    expect(pinErrorMessage(recusa('ramais.pin_belongs_to_member', 409), t)).toContain('só o podes apagar')
  })
})

describe('o meu ramal (R276)', () => {
  const meu = (pin_state: 'unset' | 'set' | 'locked', orgId = 'org-1', orgName = 'Alfa') => ({
    orgId,
    orgName,
    extension: { id: `e-${orgId}`, extension: '1004', label: '', active: true, pin_state, meeting_access_number: '8000' },
  })
  const painel = (own: ReturnType<typeof meu>[]) => renderToStaticMarkup(h(MyExtensionList, { own }))

  it('sem ramal, diz quem o atribui — e não mostra botões de PIN', async () => {
    await usar('pt-AO')
    const html = painel([])
    expect(html).toContain('Ainda não tens ramal')
    expect(html).not.toContain('<button')
  })

  it('mostra o número e o estado do PIN, com gerar e escolher — e nunca um PIN', async () => {
    await usar('pt-AO')
    const html = painel([meu('locked')])
    expect(html).toContain('Ramal 1004')
    expect(html).toContain('data-pin-state="locked"')
    expect(html).toContain('PIN bloqueado por tentativas falhadas')
    expect(botoes(html)).toEqual(['Gerar PIN novo', 'Escolher o meu PIN', 'Configurar o Linphone'])
    // À partida não há PIN revelado nem campo aberto.
    expect(html).not.toContain('data-testid="ramal-pin"')
    expect(html).not.toContain('<input')
    // Com uma só organização, o nome dela não faz falta.
    expect(html).not.toContain('Alfa')
  })

  it('com ramal em mais de uma organização, cada linha diz de qual é', async () => {
    await usar('pt-AO')
    const html = painel([meu('set'), meu('unset', 'org-2', 'Beta')])
    expect(html).toContain('Alfa')
    expect(html).toContain('Beta')
    expect([...html.matchAll(/data-pin-state="([a-z]+)"/g)].map((m) => m[1])).toEqual(['set', 'unset'])
  })

  it('nenhuma chave crua nas quatro línguas', async () => {
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      semChavesCruas(painel([]))
      semChavesCruas(painel([meu('unset'), meu('set', 'org-2', 'Beta'), meu('locked', 'org-3', 'Gama')]))
    }
    await usar('pt-AO')
  })
})

describe('«Configurar o Linphone» (R278)', async () => {
  const { LinphoneQrBody, qrErrorMessage, remainingText } = await import('../../components/LinphoneQrDialog')
  const AGORA = Date.parse('2026-10-04T10:00:00Z')
  const ticket = {
    provisioning_url: `https://meet.exemplo.ao/api/public/extension-provisioning/${'ab'.repeat(32)}`,
    expires_at: '2026-10-04T10:10:00Z',
    extension: '1004',
  }
  const corpo = (step: Parameters<typeof LinphoneQrBody>[0]['step'], now = AGORA) =>
    renderToStaticMarkup(h(LinphoneQrBody, { extension: '1004', step, now, onIssue: nada }))

  it('avisa ANTES de emitir que ler o QR troca a password SIP', async () => {
    await usar('pt-AO')
    const html = corpo({ k: 'confirm' })
    expect(html).toContain('troca a password SIP')
    expect(html).toContain('deixa de registar')
    expect(botoes(html)).toEqual(['Gerar o QR'])
    expect(html).not.toContain('extension-provisioning')
  })

  it('mostra o QR, o tempo que falta e o URL que quebra — e o aviso continua', async () => {
    await usar('pt-AO')
    const html = corpo({ k: 'shown', ticket, qr: '<svg data-qr="1"></svg>' }, AGORA + 1500)
    expect(html).toContain('data-qr="1"')
    expect(html).toContain('role="img"')
    expect(html).toContain('Válido durante mais 9:59')
    expect(html).toContain('class="linphone-qr__url')
    expect(html).toContain(ticket.provisioning_url)
    expect(html).toContain('troca a password SIP')
  })

  it('expirado, deixa de mostrar o URL e oferece outro', async () => {
    await usar('pt-AO')
    const html = corpo({ k: 'shown', ticket, qr: '<svg></svg>' }, AGORA + 601_000)
    expect(html).toContain('expirou')
    expect(html).not.toContain(ticket.provisioning_url)
    expect(botoes(html)).toEqual(['Gerar outro QR'])
  })

  it('o tempo que falta nunca é negativo', () => {
    expect(remainingText(600_000)).toBe('10:00')
    expect(remainingText(59_001)).toBe('1:00')
    expect(remainingText(-5)).toBe('0:00')
  })

  it('cada recusa da emissão tem a sua frase', async () => {
    await usar('pt-AO')
    const erro = (code: string) => new ApiError(422, { error: 'x', code }, 'x')
    expect(qrErrorMessage(erro('ramais.sip_server_missing'), i18n.t)).toContain('servidor SIP')
    expect(qrErrorMessage(erro('ramais.public_url_missing'), i18n.t)).toContain('https')
    expect(qrErrorMessage(erro('ramais.extension_inactive'), i18n.t)).toContain('inactivo')
    expect(qrErrorMessage(new Error('rede'), i18n.t)).toBe('Não foi possível gerar o QR.')
  })

  it('na consola, o botão só aparece num ramal activo', async () => {
    await usar('pt-AO')
    const html = renderToStaticMarkup(
      h(ExtensionList, {
        list: [ramal(), ramal({ id: 'e-9', extension: '1009', active: false })],
        busyId: null,
        onToggleActive: nada,
        onRegeneratePassword: nada,
        onRemove: nada,
        onGeneratePin: nada,
        onChoosePin: nada,
        onClearPin: nada,
        onConfigureLinphone: nada,
      }),
    )
    expect(botoes(linha(html, '1004'))).toContain('Configurar o Linphone')
    expect(botoes(linha(html, '1009'))).not.toContain('Configurar o Linphone')
  })

  it('nenhuma chave crua, e tudo traduzido, nas quatro línguas', async () => {
    const keys = [
      'consola.ramais.qr.botao',
      'consola.ramais.qr.titulo',
      'consola.ramais.qr.explica',
      'consola.ramais.qr.trocaPassword',
      'consola.ramais.qr.instrucoes',
      'consola.ramais.qr.validade',
      'consola.ramais.qr.expirou',
      'consola.ramais.qr.erro.semServidor',
      'consola.ramais.qr.erro.semEndereco',
      'consola.ramais.qr.erro.inactivo',
      'consola.ramais.qr.erro.generico',
    ]
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      semChavesCruas(corpo({ k: 'confirm' }))
      semChavesCruas(corpo({ k: 'error', text: 'x' }))
      semChavesCruas(corpo({ k: 'shown', ticket, qr: null }))
      semChavesCruas(corpo({ k: 'shown', ticket, qr: null }, AGORA + 700_000))
      for (const key of keys) {
        const text = i18n.t(key, { extensao: '1004', tempo: '9:59' })
        expect(text, `${lng} ${key}`).not.toBe(key)
        if (lng !== 'pt-AO') expect(text, `${lng} ${key}`).not.toBe(i18n.t(key, { extensao: '1004', tempo: '9:59', lng: 'pt-AO' }))
      }
    }
    await usar('pt-AO')
  })
})

describe('ramal automático a quem entra (R278)', () => {
  it('o interruptor e a sua dica estão traduzidos nas quatro línguas', async () => {
    const keys = ['consola.ramais.atribuir.automatico', 'consola.ramais.atribuir.automaticoDica', 'consola.ramais.atribuir.automaticoErro']
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      for (const key of keys) {
        const text = i18n.t(key)
        expect(text, `${lng} ${key}`).not.toBe(key)
        if (lng !== 'pt-AO') expect(text, `${lng} ${key}`).not.toBe(i18n.t(key, { lng: 'pt-AO' }))
      }
      // A dica aponta para o botão da acção em massa pelo nome que ele tem nessa língua.
      expect(i18n.t('consola.ramais.atribuir.automaticoDica')).toContain(i18n.t('consola.ramais.atribuir.botao'))
    }
    await usar('pt-AO')
  })
})
