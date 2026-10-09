/**
 * Os cartões da Telefonia desenhados para HTML com dados na forma do contrato.
 * Não substitui um browser (não prova layout nem temas): prova que cada cartão
 * diz o que deve — «sem medição» em vez de zero, a razão em vez de um custo
 * inventado, cabeçalhos nas tabelas — e que nenhuma chave de tradução sai crua.
 */
import { createElement as h } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import type { CallRecord, DialPlan, SipRegistration, SipSettings, TelephonyUsage, TestNumberResult, Trunk } from '../../api'

// O cliente da API lê a sessão do armazenamento ao carregar: em Node não existe.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })

await import('../../i18n')
const { default: CallsCard } = await import('./CallsCard')
const { default: DialPlanCard } = await import('./DialPlanCard')
const { default: SipCard } = await import('./SipCard')
const { default: StatusHeader } = await import('./StatusHeader')
const { default: TrunksCard } = await import('./TrunksCard')
const { default: UsageCard } = await import('./UsageCard')
const { default: OperatorWizard } = await import('./OperatorWizard')
const { RatesList } = await import('./RatesCard')
const { TrunkEditor } = await import('./TrunkDialog')
const { presetForm } = await import('./operatorPresets')
const { emptyTrunkForm, formFromTrunk } = await import('./trunkForm')
const { default: DialPlanDialog } = await import('./DialPlanDialog')
const { default: TestNumberDialog, TestResult } = await import('./TestNumberDialog')

const noop = () => {}
const semChavesCruas = (html: string) => expect(html).not.toMatch(/\b(telecom|ui|org)\.[a-zA-Z]/)

const status = (over: Partial<Trunk['status']> = {}): Trunk['status'] => ({
  state: 'up',
  reasons: [],
  channels_in_use: 12,
  channels_max: 60,
  asr: 0.62,
  asr_answered: 62,
  asr_attempts: 100,
  asr_reason: null,
  asr_window_hours: 24,
  measured_at: '2026-10-03T14:00:00Z',
  ...over,
})

const trunk = (over: Partial<Trunk>): Trunk => ({
  id: 't-unitel',
  name: 'Unitel',
  short_code: 'UNI',
  gateway_name: 'dlx-t-unitel',
  host: 'sip.unitel.example',
  port: 5061,
  transport: 'tls',
  srtp: 'mandatory',
  scope: 'national',
  role: 'primary',
  position: 0,
  prefixes: ['92', '93', '94'],
  max_channels: 60,
  enabled: true,
  register: true,
  username: 'dlx',
  password_configured: true,
  current_price_per_min: { amount: '12.5000', currency: 'AOA' },
  status: status(),
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
  ...over,
})

const registration = (over: Partial<SipRegistration> = {}): SipRegistration => ({
  state: 'healthy',
  reasons: [],
  domain: 'sip.exemplo.ao',
  sbc_host: 'sbc.exemplo.ao',
  transport: 'tls',
  srtp: 'mandatory',
  sbc: { software: 'kamailio', version: '5.8.2', uptime_secs: 3600 },
  sbc_error: null,
  media: null,
  media_error: 'media_server_unreachable',
  channels: { in_use: 36, max: 120 },
  sessions_active: 18,
  trunks: { total: 4, up: 3, degraded: 1, down: 0, unknown: 0 },
  quality: { calls: 210, window_hours: 24, jitter_ms: 6, loss_pct: 0, mos: 4.31, reason: null },
  codecs_configured: ['OPUS', 'PCMA'],
  codecs_offered: ['OPUS', 'PCMA'],
  measured_at: '2026-10-03T14:00:00Z',
  ...over,
})

describe('cabeçalho do SBC', () => {
  it('com medições mostra-as — e uma perda de 0 é zero medido, não «sem medição»', () => {
    const html = renderToStaticMarkup(h(StatusHeader, { registration: registration() }))
    expect(html).toContain('SBC saudável')
    expect(html).toContain('36')
    expect(html).toContain('de 120')
    expect(html).toContain('3 activas')
    expect(html).toContain('1 degradada')
    expect(html).toContain('6,0 ms')
    expect(html).toContain('0,0%')
    expect(html).toContain('4,31')
    expect(html).not.toContain('sem medição')
    semChavesCruas(html)
  })

  it('sem medições diz «sem medição» com a razão, e não escreve zeros', () => {
    const html = renderToStaticMarkup(
      h(StatusHeader, { registration: registration({
          state: 'degraded',
          reasons: ['media_server_unreachable'],
          channels: { in_use: null, max: 120 },
          quality: { calls: 0, window_hours: 24, jitter_ms: null, loss_pct: null, mos: null, reason: 'no_calls_in_window' },
        }) }),
    )
    expect(html).toContain('SBC degradado')
    expect(html).toContain('servidor de media inacessível')
    expect(html.match(/sem medição/g)?.length).toBe(4)
    expect(html).toContain('sem chamadas na janela')
    expect(html).not.toContain('0 ms')
    expect(html).not.toContain('0,0')
    semChavesCruas(html)
  })

  it('um estado e uma razão que a consola não conhece saem tal qual', () => {
    const html = renderToStaticMarkup(h(StatusHeader, { registration: registration({ state: 'draining', reasons: ['operator_maintenance'] }) }))
    expect(html).toContain('draining')
    expect(html).toContain('operator_maintenance')
  })
})

describe('operadoras', () => {
  const list = [
    trunk({ id: 't-afr', name: 'Africell', short_code: 'AFR', position: 1, current_price_per_min: null, status: status({ state: 'unknown', reasons: ['no_measurement'], channels_in_use: null, asr: null, asr_answered: 0, asr_attempts: 0, asr_reason: 'no_calls_in_window' }) }),
    trunk({}),
  ]

  const escritas = { orgId: 'org-1', mutate: noop, onChanged: noop }

  it('saem pela posição, com estado em texto, preço formatado e ASR com base', () => {
    const html = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: list, next: null } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html.indexOf('Unitel')).toBeLessThan(html.indexOf('Africell'))
    expect(html).toContain('sip.unitel.example')
    expect(html).toContain('Activa')
    expect(html).toContain('12,50 Kz')
    expect(html).toContain('62%')
    expect(html).toContain('62 de 100 em 24 h')
    expect(html).toContain('92 · 93 · 94')
    // A segunda não mediu nada: razão por extenso, nenhum zero.
    expect(html).toContain('Estado desconhecido')
    expect(html).toContain('ainda sem medição')
    expect(html).toContain('sem chamadas na janela')
    expect(html).toContain('sem preço em vigor')
    expect(html).not.toContain('0%')
    expect(html).not.toContain('Carregar mais')
    semChavesCruas(html)
  })

  it('com cursor oferece «carregar mais»; lista vazia diz que está vazia', () => {
    const more = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: list, next: 'abc' } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(more).toContain('Carregar mais')
    const empty = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: [], next: null } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(empty).toContain('Ainda não há operadoras')
  })

  it('a ordem muda-se sem rato: cada linha tem «subir» e «descer» com nome, e as pontas ficam inactivas', () => {
    const html = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: list, next: null } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    for (const nome of ['Unitel', 'Africell']) {
      expect(html).toContain(`aria-label="Subir ${nome} no encaminhamento"`)
      expect(html).toContain(`aria-label="Descer ${nome} no encaminhamento"`)
    }
    // A primeira não sobe e a última não desce.
    expect(html).toMatch(/<button[^>]*aria-label="Subir Unitel no encaminhamento"[^>]*disabled=""|<button[^>]*disabled=""[^>]*aria-label="Subir Unitel no encaminhamento"/)
    expect(html).toMatch(/<button[^>]*aria-label="Descer Africell no encaminhamento"[^>]*disabled=""|<button[^>]*disabled=""[^>]*aria-label="Descer Africell no encaminhamento"/)
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*aria-label="Descer Unitel no encaminhamento"/)
    // A pega de arrastar é só para o rato: fica fora da árvore de acessibilidade.
    expect(html).toMatch(/class="tel-grip"[^>]*aria-hidden="true"/)
    expect(html).toContain('role="status"')
    semChavesCruas(html)
  })

  it('sem a lista inteira não se reordena — a ordem manda-se toda de uma vez', () => {
    const html = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: list, next: 'abc' } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html).not.toContain('aria-label="Subir')
    expect(html).not.toContain('aria-label="Descer')
    expect(html).not.toContain('tel-grip')
    expect(html).toContain('Carregue todas as operadoras')
  })

  it('cada operadora tem editar, preços, desactivar e apagar; o cartão cria por formulário ou por assistente', () => {
    const html = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: [trunk({}), trunk({ id: 't-off', name: 'Movicel', position: 2, enabled: false })], next: null } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html).toContain('Novo tronco SIP')
    expect(html).toContain('Ligar operadora móvel')
    expect(html).toContain('aria-label="Acções de Unitel"')
    expect(html).toContain('Preços')
    expect(html).toContain('Apagar')
    // Activa oferece «Desactivar»; desactivada oferece «Activar» e diz que está desactivada.
    expect(html).toContain('>Desactivar<')
    expect(html).toContain('>Activar<')
    expect(html).toContain('Desactivada')
    // A password nunca aparece numa listagem.
    expect(html).not.toMatch(/password/i)
    semChavesCruas(html)
  })

  it('sem operadoras continua a poder criar a primeira', () => {
    const html = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'ready', d: { items: [], next: null } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html).toContain('Ainda não há operadoras')
    expect(html).toContain('Ligar operadora móvel')
  })

  it('um erro mostra o erro — não «sem operadoras»', () => {
    const html = renderToStaticMarkup(h(TrunksCard, { ...escritas, state: { s: 'error', msg: 'Falha de rede' }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html).toContain('Falha de rede')
    expect(html).toContain('role="alert"')
    expect(html).not.toContain('Ainda não há operadoras')
  })
})

describe('formulário de operadora', () => {
  const base = { orgId: 'org-1', onDone: noop, onCancel: noop }
  /** O campo tal como sai, pelo sufixo do id (os ids levam o prefixo do useId). */
  const campo = (html: string, nome: string) => new RegExp(`<input[^>]*id="[^"]*-${nome}"[^>]*>`).exec(html)?.[0] ?? ''

  it('criar: todos os campos têm rótulo, o preço é opcional e nada sai em chave crua', () => {
    const html = renderToStaticMarkup(h(TrunkEditor, { ...base, initial: emptyTrunkForm() }))
    for (const rotulo of ['Nome', 'Código curto', 'Host do SBC', 'Porta', 'Transporte', 'SRTP', 'Âmbito', 'Prefixos', 'Limite de canais', 'Registar na operadora', 'Utilizador', 'Password', 'Preço por minuto', 'Moeda', 'Operadora activa']) {
      expect(html, rotulo).toContain(rotulo)
    }
    expect(html.match(/<label class="dx-field__label" for="/g)?.length).toBe(13)
    expect(html).toContain('Criar operadora')
    semChavesCruas(html)
  })

  it('editar: a password sai VAZIA e diz que vazio é manter; não há preço (tem ecrã próprio)', () => {
    const k = trunk({ password_configured: true })
    const html = renderToStaticMarkup(h(TrunkEditor, { ...base, trunk: k, initial: formFromTrunk(k) }))
    expect(campo(html, 'password')).toContain('type="password"')
    expect(campo(html, 'password')).toContain('value=""')
    expect(html).toContain('Deixe vazio para a manter')
    expect(html).not.toContain('Preço por minuto')
    expect(html).toContain('Guardar')
    expect(campo(html, 'host')).toContain('value="sip.unitel.example"')
  })

  it('SRTP sem TLS avisa JÁ, antes de qualquer envio, e marca o campo', () => {
    const html = renderToStaticMarkup(h(TrunkEditor, { ...base, initial: { ...emptyTrunkForm(), transport: 'udp', srtp: 'mandatory' } }))
    expect(html).toContain('SRTP exige transporte TLS')
    expect(html).toMatch(/<select[^>]*aria-invalid="true"/)
    // Os outros erros esperam pelo envio: um formulário acabado de abrir não acusa o que está vazio.
    expect(html).not.toContain('Indique o endereço do SBC')
  })

  it('UDP sem SRTP é aceite, com o aviso de rede privada', () => {
    const html = renderToStaticMarkup(h(TrunkEditor, { ...base, initial: { ...emptyTrunkForm(), transport: 'udp', srtp: 'off' } }))
    expect(html).toContain('rede privada')
    expect(html).not.toContain('SRTP exige transporte TLS')
    const seguro = renderToStaticMarkup(h(TrunkEditor, { ...base, initial: emptyTrunkForm() }))
    expect(seguro).not.toContain('rede privada')
  })

  it('assistente: quatro escolhas, e o passo final traz o host VAZIO e os prefixos como sugestão', () => {
    const passo1 = renderToStaticMarkup(h(OperatorWizard, { orgId: 'org-1', onClose: noop, onDone: noop }))
    expect(passo1.match(/type="radio"/g)?.length).toBe(4)
    for (const nome of ['Unitel', 'Africell', 'Movicel', 'Outra operadora']) expect(passo1).toContain(nome)
    expect(passo1).toContain('Passo 1 de 3')
    expect(passo1).toContain('role="dialog"')
    semChavesCruas(passo1)

    const fim = renderToStaticMarkup(h(TrunkEditor, { ...base, initial: presetForm('unitel'), prefixesSuggested: true }))
    expect(campo(fim, 'host')).toContain('value=""')
    expect(campo(fim, 'max_channels')).toContain('value=""')
    expect(campo(fim, 'name')).toContain('value="Unitel"')
    expect(campo(fim, 'port')).toContain('value="5061"')
    expect(fim).toContain('Sugestão — confirmar com a operadora.')
  })
})

describe('câmbio', () => {
  it('a taxa é texto formatado, sem passar por número', () => {
    const html = renderToStaticMarkup(h(RatesList, { items: [{ id: 'r1', currency: 'USD', aoa_per_unit: '912.500000', valid_from: '2026-10-01T00:00:00Z', created_at: '2026-10-01T00:00:00Z' }] }))
    expect(html).toContain('1 USD = 912,50 Kz')
    semChavesCruas(html)
  })
})

describe('plano de marcação', () => {
  const plan: DialPlan = {
    rules: [
      { pattern: '9XXXXXXXX', description: 'Móveis nacionais', action: 'external', trunk_id: 't-unitel', fallback_trunk_id: 't-desconhecido-0001', record: true },
      { pattern: '1XX', description: '', action: 'extension', trunk_id: null, fallback_trunk_id: null, record: false },
    ],
    emergency_numbers: ['112', '113', '115'],
    version: 3,
    updated_at: '2026-10-01T00:00:00Z',
  }

  it('é uma tabela com cabeçalhos de coluna e de linha, e os números de emergência', () => {
    const html = renderToStaticMarkup(h(DialPlanCard, { orgId: 'org-1', state: { s: 'ready', d: plan }, reload: noop, trunks: [trunk({})] }))
    expect(html.match(/<th scope="col"/g)?.length).toBe(6)
    expect(html.match(/<th scope="row"/g)?.length).toBe(2)
    expect(html).toContain('9XXXXXXXX')
    expect(html).toContain('Móveis nacionais')
    expect(html).toContain('Chamada externa')
    expect(html).toContain('Unitel')
    // Uma operadora que não está na lista carregada mostra o identificador, não um nome inventado.
    expect(html).toContain('t-descon')
    expect(html).toContain('Ramal interno')
    for (const n of plan.emergency_numbers) expect(html).toContain(`>${n}<`)
    semChavesCruas(html)
  })

  it('oferece «Editar plano» e «Testar número»', () => {
    const html = renderToStaticMarkup(h(DialPlanCard, { orgId: 'org-1', state: { s: 'ready', d: plan }, reload: noop, trunks: [trunk({})] }))
    expect(html).toContain('Editar plano')
    expect(html).toContain('Testar número')
    semChavesCruas(html)
  })

  it('o editor: uma regra por bloco, com mover e remover por nome, e a emergência só para ler', () => {
    const comEmergencia: DialPlan = {
      ...plan,
      rules: [
        { pattern: '9XXXXXXXX', description: 'Móveis nacionais', action: 'external', trunk_id: 't-unitel', fallback_trunk_id: null, record: true, emergency: false },
        { pattern: '1XX', description: 'Ramais', action: 'extension', trunk_id: null, fallback_trunk_id: null, record: false, emergency: false },
        { pattern: '112', description: 'Emergência', action: 'external', trunk_id: 't-unitel', fallback_trunk_id: null, record: false, emergency: true },
      ],
    }
    const html = renderToStaticMarkup(h(DialPlanDialog, { orgId: 'org-1', plan: comEmergencia, trunks: [trunk({}), trunk({ id: 't-off', name: 'Movicel', position: 1, enabled: false })], onClose: noop, onSaved: noop }))
    expect(html).toContain('role="dialog"')
    expect(html.match(/<li class="tel-rule"/g)?.length).toBe(3)
    for (const n of [1, 2, 3]) {
      expect(html).toContain(`aria-label="Subir a regra ${n}"`)
      expect(html).toContain(`aria-label="Descer a regra ${n}"`)
      expect(html).toContain(`aria-label="Remover a regra ${n}"`)
    }
    // A primeira não sobe; a última não desce.
    expect(html).toMatch(/aria-label="Subir a regra 1"[^>]*disabled=""|disabled=""[^>]*aria-label="Subir a regra 1"/)
    expect(html).toMatch(/aria-label="Descer a regra 3"[^>]*disabled=""|disabled=""[^>]*aria-label="Descer a regra 3"/)
    // Só as regras externas escolhem operadora e reserva (duas, não três).
    expect(html.match(/>Escolher operadora</g)?.length).toBe(2)
    expect(html.match(/>Sem reserva</g)?.length).toBe(2)
    expect(html).toContain('Movicel (desactivada)')
    // A regra de emergência não muda de acção nem se grava.
    expect(html).toContain('sai sempre por uma operadora e nunca é gravada')
    expect(html.match(/<select[^>]*disabled=""/g)?.length).toBe(1)
    expect(html.match(/<input type="checkbox"[^>]*disabled=""/g)?.length).toBe(1)
    // Os números de emergência mostram-se como etiquetas — não há campo para os editar.
    for (const n of comEmergencia.emergency_numbers) expect(html).toContain(`>${n}<`)
    const blocoEmergencia = html.slice(html.indexOf('tel-emergency--box'), html.indexOf('tel-form__foot'))
    expect(blocoEmergencia).toContain('>113<')
    expect(blocoEmergencia).not.toMatch(/<input|<select|<button/)
    expect(html).toContain('não se editam aqui')
    expect(html).toContain('Gravar plano')
    semChavesCruas(html)
  })

  it('o editor de um plano vazio deixa acrescentar a primeira regra', () => {
    const html = renderToStaticMarkup(h(DialPlanDialog, { orgId: 'org-1', plan: { ...plan, rules: [] }, trunks: [], onClose: noop, onSaved: noop }))
    expect(html).toContain('acrescente a primeira')
    expect(html).toContain('Acrescentar regra')
    expect(html).not.toContain('tel-rule"')
  })

  it('sem regras diz que não há regras, e a emergência continua visível', () => {
    const html = renderToStaticMarkup(h(DialPlanCard, { orgId: 'org-1', state: { s: 'ready', d: { ...plan, rules: [] } }, reload: noop, trunks: [] }))
    expect(html).toContain('ainda não tem regras')
    expect(html).not.toContain('<table')
    expect(html).toContain('>112<')
  })
})

describe('testar número', () => {
  const resultado = (over: Partial<TestNumberResult> = {}): TestNumberResult => ({
    dialed: '923000000',
    e164: '+244923000000',
    outcome: 'route',
    matched_rule: { position: 0, pattern: '9XXXXXXXX', description: 'Móveis nacionais' },
    action: 'external',
    trunk: { id: 't-unitel', name: 'Unitel', short_code: 'UNI' },
    fallbacks: [{ id: 't-afr', name: 'Africell', short_code: 'AFR' }],
    recorded: true,
    emergency: false,
    overridden_rule_position: null,
    estimated_price_per_min: { amount: '12.5000', currency: 'AOA' },
    price_reason: null,
    ...over,
  })

  it('o diálogo diz que não liga a ninguém', () => {
    const html = renderToStaticMarkup(h(TestNumberDialog, { orgId: 'org-1', onClose: noop }))
    expect(html).toContain('Não liga a ninguém')
    expect(html).toContain('type="tel"')
    semChavesCruas(html)
  })

  it('uma chamada que sai: regra (a contar de 1), operadora, reservas, preço, gravação', () => {
    const html = renderToStaticMarkup(h(TestResult, { result: resultado() }))
    expect(html).toContain('Sai por uma operadora')
    expect(html).toContain('+244923000000')
    expect(html).toContain('#1 · 9XXXXXXXX')
    expect(html).toContain('Móveis nacionais')
    expect(html).toContain('Unitel (UNI)')
    expect(html).toContain('Africell')
    expect(html).toContain('12,50 Kz/min')
    expect(html).toContain('Sim')
    semChavesCruas(html)
  })

  it('emergência sem regra: diz que sai sempre, e não «nenhuma regra casa»', () => {
    const html = renderToStaticMarkup(h(TestResult, { result: resultado({ dialed: '112', e164: null, matched_rule: null, emergency: true, recorded: false, fallbacks: [], overridden_rule_position: 2 }) }))
    expect(html).toContain('Emergência')
    expect(html).toContain('sai sempre, não depende das regras')
    expect(html).not.toContain('Nenhuma regra casa.')
    expect(html).toContain('A regra #3 casaria primeiro')
    expect(html).toContain('sem reserva')
  })

  it('sem regra e sem preço: diz a razão, nunca um zero', () => {
    const html = renderToStaticMarkup(h(TestResult, { result: resultado({ outcome: 'no_match', matched_rule: null, action: null, trunk: null, fallbacks: [], recorded: false, estimated_price_per_min: null, price_reason: 'not_external' }) }))
    expect(html).toContain('Nenhuma regra casa')
    expect(html).toContain('não sai por uma operadora')
    expect(html).not.toContain('0,00')
    expect(html).not.toContain('Kz')
  })

  it('bloqueado mostra-se como bloqueado, e um desfecho ou razão novos saem tal qual', () => {
    expect(renderToStaticMarkup(h(TestResult, { result: resultado({ outcome: 'blocked', trunk: null, estimated_price_per_min: null, price_reason: 'not_external' }) }))).toContain('Bloqueado')
    const novo = renderToStaticMarkup(h(TestResult, { result: resultado({ outcome: 'queued', estimated_price_per_min: null, price_reason: 'tariff_pending' }) }))
    expect(novo).toContain('queued')
    expect(novo).toContain('tariff_pending')
  })
})

describe('registo SIP', () => {
  const settings: SipSettings = { configured: true, domain: 'sip.exemplo.ao', sbc_host: 'sbc.exemplo.ao', transport: 'tls', srtp: 'mandatory', username: 'dlx-org', password_configured: true, codecs: ['OPUS', 'PCMA'], updated_at: null }

  it('mostra as definições, diz só SE há password, e não tem botões', () => {
    const html = renderToStaticMarkup(h(SipCard, { settings: settings, registration: registration() }))
    expect(html).toContain('sip.exemplo.ao')
    expect(html).toContain('TLS')
    expect(html).toContain('Obrigatório')
    expect(html).toContain('OPUS · PCMA')
    expect(html).toContain('dlx-org')
    expect(html).toContain('configurada')
    expect(html).toContain('kamailio 5.8.2')
    // O servidor de media não respondeu: a razão, não uma versão.
    expect(html).toContain('servidor de media inacessível')
    expect(html).not.toContain('<button')
    semChavesCruas(html)
  })

  it('por configurar: cada campo diz «não definido»', () => {
    const off: SipSettings = { configured: false, domain: null, sbc_host: null, transport: null, srtp: null, username: null, password_configured: false, codecs: [], updated_at: null }
    const html = renderToStaticMarkup(h(SipCard, { settings: off, registration: registration({ sbc: null, sbc_error: null, media: null, media_error: 'not_configured', codecs_offered: [] }) }))
    expect(html).toContain('ainda não foram configuradas')
    expect(html.match(/não definido/g)?.length).toBe(6)
    expect(html).toContain('não configurada')
  })
})

describe('chamadas externas', () => {
  const call = (over: Partial<CallRecord>): CallRecord => ({
    id: 'c1',
    direction: 'outbound',
    from_number: '+244222000111',
    to_number: '+244923447108',
    destination_label: 'Móveis nacionais',
    outcome: 'answered',
    hangup_cause: 'NORMAL_CLEARING',
    started_at: '2026-10-03T10:00:00Z',
    answered_at: '2026-10-03T10:00:05Z',
    ended_at: '2026-10-03T10:02:10Z',
    duration_secs: 130,
    billsec: 125,
    emergency: false,
    recorded: true,
    trunk_id: 't-unitel',
    trunk_name: 'Unitel',
    cost: { amount: '37.5000', currency: 'AOA' },
    cost_reason: null,
    ...over,
  })

  it('custo formatado quando existe; a razão quando não existe — nunca zero', () => {
    const items = [call({}), call({ id: 'c2', direction: 'inbound', outcome: 'no_answer', billsec: 0, recorded: false, trunk_name: null, cost: null, cost_reason: 'inbound_not_billed' })]
    const html = renderToStaticMarkup(h(CallsCard, { state: { s: 'ready', d: { items, next: 'cursor' } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html.match(/<th scope="col"/g)?.length).toBe(7)
    expect(html).toContain('Efectuada')
    expect(html).toContain('+244923447108')
    expect(html).toContain('37,50 Kz')
    expect(html).toContain('2:05')
    expect(html).toContain('Gravada')
    expect(html).toContain('Recebida')
    expect(html).toContain('+244222000111')
    expect(html).toContain('Sem resposta')
    expect(html).toContain('recebida, não se cobra')
    expect(html).not.toContain('0,00 Kz')
    expect(html).toContain('Carregar mais')
    semChavesCruas(html)
  })

  it('a última página — sem cursor — não oferece «carregar mais»', () => {
    const html = renderToStaticMarkup(h(CallsCard, { state: { s: 'ready', d: { items: [call({})], next: null } }, reload: noop, loadMore: noop, busy: false, err: '' }))
    expect(html).not.toContain('Carregar mais')
  })

  it('falhar ao carregar mais mantém as linhas e mostra o erro', () => {
    const html = renderToStaticMarkup(h(CallsCard, { state: { s: 'ready', d: { items: [call({})], next: 'cursor' } }, reload: noop, loadMore: noop, busy: false, err: 'Falha de rede' }))
    expect(html).toContain('+244923447108')
    expect(html).toContain('Falha de rede')
  })
})

describe('consumo do mês', () => {
  const usage: TelephonyUsage = {
    month: '2026-10',
    timezone: 'Africa/Luanda',
    calls: 1204,
    minutes: 4812,
    unpriced_calls: 0,
    totals: [{ amount: '184620.0000', currency: 'AOA' }],
    total_aoa: { amount: '184620.0000', currency: 'AOA' },
    total_aoa_reason: null,
    by_trunk: [
      { trunk_id: 't-unitel', trunk_name: 'Unitel', calls: 800, minutes: 2983, cost: [{ amount: '114464.4000', currency: 'AOA' }], cost_aoa: { amount: '114464.4000', currency: 'AOA' }, share_pct: 62 },
      { trunk_id: null, trunk_name: null, calls: 4, minutes: 9, cost: [], cost_aoa: null, share_pct: null },
    ],
  }

  it('total, minutos e repartição por operadora', () => {
    const html = renderToStaticMarkup(h(UsageCard, { state: { s: 'ready', d: usage }, reload: noop }))
    expect(html).toMatch(/184[\s.]?620,00 Kz/)
    expect(html).toMatch(/4[\s.]?812 minutos/)
    expect(html).toContain('Unitel')
    expect(html).toContain('62,0%')
    expect(html).toMatch(/114[\s.]?464,40 Kz/)
    expect(html).toContain('Sem operadora')
    expect(html).toContain('sem quota')
    semChavesCruas(html)
  })

  it('sem total em kwanzas mostra a razão e os totais por moeda — não um zero', () => {
    const html = renderToStaticMarkup(
      h(UsageCard, { state: { s: 'ready', d: { ...usage, total_aoa: null, total_aoa_reason: 'missing_exchange_rate', unpriced_calls: 3, totals: [{ amount: '1200.0000', currency: 'AOA' }, { amount: '14.2500', currency: 'USD' }] } }, reload: noop }),
    )
    expect(html).toContain('Total em kwanzas indisponível')
    expect(html).toContain('falta o câmbio para kwanzas')
    expect(html).toContain('14,25 USD')
    expect(html).toContain('3 chamadas sem preço')
    expect(html).not.toContain('tel-usage__big')
  })
})
