/**
 * A lógica do formulário de operadora, dos preços e do assistente. As formas
 * dos erros são as que o servidor devolveu de facto a 2026-10-03 (corpo
 * `{ code, details: [{ field, description }], error }`).
 */
import { describe, expect, it, vi } from 'vitest'
import type { Trunk } from '../../api'

// O cliente da API lê a sessão do armazenamento ao carregar: em Node não existe.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })

const { ApiError } = await import('../../api')
const {
  createBody,
  dateOrAmountField,
  decimalText,
  emptyTrunkForm,
  errorCode,
  errorFields,
  errorKey,
  formFromTrunk,
  isAmountText,
  isCleartext,
  isPrefix,
  isRateText,
  localToIso,
  moneyFrom,
  moveItem,
  moveTo,
  patchBody,
  prefixProblem,
  splitPrefixes,
  srtpNeedsTls,
  trunkErrorField,
  validateTrunkForm,
} = await import('./trunkForm')
const { ASK_OPERATOR, OPERATORS, presetForm } = await import('./operatorPresets')

const trunk = (over: Partial<Trunk> = {}): Trunk => ({
  id: 't1',
  name: 'Unitel',
  short_code: 'UNI',
  gateway_name: 'dlx-t1',
  host: 'sbc.operadora.example',
  port: 5061,
  transport: 'tls',
  srtp: 'mandatory',
  scope: 'national',
  role: 'primary',
  position: 0,
  prefixes: ['92', '93'],
  max_channels: 30,
  enabled: true,
  register: false,
  username: 'dlx',
  password_configured: true,
  current_price_per_min: null,
  status: { state: 'unknown', reasons: [], channels_max: 30, asr_answered: 0, asr_attempts: 0, asr_window_hours: 24, measured_at: '2026-10-03T00:00:00Z' },
  created_at: '2026-10-01T00:00:00Z',
  updated_at: '2026-10-01T00:00:00Z',
  ...over,
})

const recusa = (status: number, code: string, details: { field: string; description: string }[] = []) =>
  new ApiError(status, { code, details, error: 'texto do servidor', request_id: 'x' }, 'texto do servidor')

const valido = () => ({ ...emptyTrunkForm(), name: 'Unitel', short_code: 'UNI', host: 'sbc.operadora.example', max_channels: '30' })

describe('dinheiro: valida-se a forma, em texto', () => {
  it('aceita decimal positivo com até 4 casas, com vírgula ou ponto', () => {
    for (const ok of ['12', '9.40', '9,40', '0.0125', '0', ' 12.5 ']) expect(isAmountText(ok), ok).toBe(true)
    for (const mau of ['', '-1', '1.23456', '1e3', '1.', '.5', '1,2,3', '12 Kz', 'abc', '1 000']) expect(isAmountText(mau), mau).toBe(false)
  })

  it('a taxa de câmbio tem até 6 casas e é maior do que zero', () => {
    for (const ok of ['912.50', '912,5', '1', '0.000001']) expect(isRateText(ok), ok).toBe(true)
    for (const mau of ['', '0', '0.000000', '0,0', '-5', '1.2345678', '1234567890', 'abc']) expect(isRateText(mau), mau).toBe(false)
  })

  it('o texto segue para o servidor tal como foi escrito — só a vírgula vira ponto', () => {
    expect(decimalText(' 9,40 ')).toBe('9.40')
    // Um valor que um float não representa chega inteiro.
    expect(moneyFrom('123456789012345,0001', 'AOA')).toEqual({ amount: '123456789012345.0001', currency: 'AOA' })
    expect(typeof moneyFrom('0,1', 'USD').amount).toBe('string')
  })

  it('a data de início: vazio é «agora» (omite-se), lixo é erro, o resto vai em RFC 3339', () => {
    expect(localToIso('')).toBeUndefined()
    expect(localToIso('não é data')).toBeNull()
    expect(localToIso('2027-01-01T00:00')).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/)
  })

  it('uma recusa de preço ou taxa vai para o campo certo', () => {
    expect(dateOrAmountField('telephony.price_backdated')).toBe('valid_from')
    expect(dateOrAmountField('telephony.price_exists')).toBe('valid_from')
    expect(dateOrAmountField('telephony.rate_exists')).toBe('valid_from')
    expect(dateOrAmountField('telephony.invalid_amount')).toBe('amount')
    expect(dateOrAmountField('telephony.invalid_rate')).toBe('amount')
    expect(dateOrAmountField('telephony.trunk_in_use')).toBeNull()
    expect(dateOrAmountField(null)).toBeNull()
  })
})

describe('prefixos', () => {
  it('separam-se por vírgula, espaço, ponto e vírgula ou «·», sem repetidos', () => {
    expect(splitPrefixes('92, 93;94 · 92  +244')).toEqual(['92', '93', '94', '+244'])
    expect(splitPrefixes('  ')).toEqual([])
  })

  it('a regra é a do servidor: dígitos, «+» opcional no início, até 8 caracteres', () => {
    for (const ok of ['9', '92', '00', '+', '+244', '12345678']) expect(isPrefix(ok), ok).toBe(true)
    for (const mau of ['9a', '++1', '9+', '123456789', '-1', '9.2']) expect(isPrefix(mau), mau).toBe(false)
  })

  it('diz QUAL prefixo está mal, e recusa mais de 20', () => {
    expect(prefixProblem('92, 93')).toBeNull()
    expect(prefixProblem('')).toBeNull()
    expect(prefixProblem('92, 9x')).toEqual({ key: 'prefixoInvalido', prefix: '9x' })
    expect(prefixProblem(Array.from({ length: 21 }, (_, i) => String(100 + i)).join(','))).toEqual({ key: 'prefixosDemais' })
  })
})

describe('SRTP exige TLS — avisa-se no formulário, antes de enviar', () => {
  it('só «off» dispensa o TLS', () => {
    expect(srtpNeedsTls('tls', 'mandatory')).toBe(false)
    expect(srtpNeedsTls('udp', 'off')).toBe(false)
    expect(srtpNeedsTls('udp', 'mandatory')).toBe(true)
    expect(srtpNeedsTls('tcp', 'optional')).toBe(true)
  })

  it('a validação local trava a combinação e põe o erro no SRTP', () => {
    expect(validateTrunkForm({ ...valido(), transport: 'udp', srtp: 'mandatory' }, 'create').srtp).toEqual({ key: 'srtpExigeTls' })
    expect(validateTrunkForm({ ...valido(), transport: 'udp', srtp: 'off' }, 'create').srtp).toBeUndefined()
  })

  it('sinalização ou voz em claro pede rede privada', () => {
    expect(isCleartext('tls', 'mandatory')).toBe(false)
    expect(isCleartext('tls', 'optional')).toBe(false)
    expect(isCleartext('udp', 'off')).toBe(true)
    expect(isCleartext('tls', 'off')).toBe(true)
  })
})

describe('validação do formulário', () => {
  it('um formulário completo passa', () => {
    expect(validateTrunkForm(valido(), 'create')).toEqual({})
  })

  it('host e canais são obrigatórios — o formulário vazio não segue', () => {
    const e = validateTrunkForm(emptyTrunkForm(), 'create')
    expect(e.host?.key).toBe('hostObrigatorio')
    expect(e.max_channels?.key).toBe('canaisObrigatorio')
    expect(e.name?.key).toBe('nome')
    expect(e.short_code?.key).toBe('sigla')
  })

  it('apanha a forma errada de cada campo', () => {
    const e = (over: object) => validateTrunkForm({ ...valido(), ...over }, 'create')
    for (const host of ['sip://x', 'a b', 'x/y', '-x', 'a..b']) expect(e({ host }).host?.key, host).toBe('host')
    for (const port of ['0', '65536', 'abc', '']) expect(e({ port }).port?.key, port).toBe('porta')
    for (const max_channels of ['0', '10001', '1.5']) expect(e({ max_channels }).max_channels?.key, max_channels).toBe('canais')
    for (const short_code of ['U', 'UN-I', 'UNITE']) expect(e({ short_code }).short_code?.key, short_code).toBe('sigla')
    expect(e({ prefixes: '92, 9x' }).prefixes).toEqual({ key: 'prefixoInvalido', vars: { prefixo: '9x' } })
    expect(e({ price_amount: '1.23456' }).price?.key).toBe('preco')
    expect(e({ price_amount: '' }).price).toBeUndefined()
  })

  it('ao editar não há preço para validar (os preços têm ecrã próprio)', () => {
    expect(validateTrunkForm({ ...valido(), price_amount: 'lixo' }, 'edit').price).toBeUndefined()
  })
})

describe('criar', () => {
  it('manda o registo explícito, e a password e o preço só se foram escritos', () => {
    const body = createBody({ ...valido(), short_code: 'uni', host: ' SBC.Operadora.Example ', prefixes: '92, 93' })
    expect(body).toEqual({
      name: 'Unitel',
      short_code: 'UNI',
      host: 'sbc.operadora.example',
      max_channels: 30,
      port: 5061,
      transport: 'tls',
      srtp: 'mandatory',
      scope: 'national',
      prefixes: ['92', '93'],
      enabled: true,
      register: false,
      username: '',
    })
    expect('password' in body).toBe(false)
    expect('price_per_min' in body).toBe(false)
  })

  it('com password e preço, o preço vai em texto', () => {
    const body = createBody({ ...valido(), password: 'segredo', price_amount: '9,40', price_currency: 'AOA' })
    expect(body.password).toBe('segredo')
    expect(body.price_per_min).toEqual({ amount: '9.40', currency: 'AOA' })
  })
})

describe('editar: o PATCH leva só o que muda', () => {
  it('sem alterações o corpo é vazio — e a password não vem do servidor nem volta para ele', () => {
    const k = trunk()
    const f = formFromTrunk(k)
    expect(f.password).toBe('')
    expect(patchBody(k, f)).toEqual({})
  })

  it('password vazia = manter: nunca vai no PATCH, nem a null nem a vazio', () => {
    const k = trunk()
    const body = patchBody(k, { ...formFromTrunk(k), name: 'Unitel SA' })
    expect(body).toEqual({ name: 'Unitel SA' })
    expect('password' in body).toBe(false)
  })

  it('password escrita vai, e só ela', () => {
    const k = trunk()
    expect(patchBody(k, { ...formFromTrunk(k), password: 'nova' })).toEqual({ password: 'nova' })
  })

  it('cada campo alterado aparece, com a forma normalizada', () => {
    const k = trunk()
    const body = patchBody(k, {
      ...formFromTrunk(k),
      short_code: 'un2',
      host: 'SBC2.operadora.example',
      port: '5060',
      transport: 'udp',
      srtp: 'off',
      scope: 'international',
      prefixes: '92, 93, 94',
      max_channels: '60',
      register: true,
      username: 'outro',
      enabled: false,
    })
    expect(body).toEqual({
      short_code: 'UN2',
      host: 'sbc2.operadora.example',
      port: 5060,
      transport: 'udp',
      srtp: 'off',
      scope: 'international',
      prefixes: ['92', '93', '94'],
      max_channels: 60,
      register: true,
      username: 'outro',
      enabled: false,
    })
  })

  it('reescrever os mesmos prefixos com outros separadores não é uma alteração', () => {
    const k = trunk()
    expect(patchBody(k, { ...formFromTrunk(k), prefixes: '92;93' })).toEqual({})
    expect(patchBody(k, { ...formFromTrunk(k), prefixes: '93, 92' })).toEqual({ prefixes: ['93', '92'] })
  })
})

describe('recusas do servidor: pelo código e pelo campo, nunca pelo texto', () => {
  it('lê o código e os campos de `details[]`', () => {
    const e = recusa(400, 'telephony.srtp_requires_tls', [{ field: 'srtp', description: 'telephony.srtp_requires_tls' }])
    expect(errorCode(e)).toBe('telephony.srtp_requires_tls')
    expect(errorFields(e)).toEqual(['srtp'])
    expect(errorCode(new Error('rede'))).toBeNull()
    expect(errorFields(new Error('rede'))).toEqual([])
    expect(errorFields(new ApiError(500, 'texto', 'texto'))).toEqual([])
  })

  it('cada recusa real marca o campo certo', () => {
    const casos: [number, string, string[], string | null][] = [
      [400, 'telephony.srtp_requires_tls', ['srtp'], 'srtp'],
      [400, 'telephony.trunk_host_refused', ['host'], 'host'],
      [400, 'telephony.invalid_trunk_host', ['host'], 'host'],
      [400, 'telephony.invalid_short_code', ['short_code'], 'short_code'],
      [400, 'telephony.invalid_prefixes', ['prefixes'], 'prefixes'],
      [400, 'telephony.invalid_max_channels', ['max_channels'], 'max_channels'],
      [400, 'telephony.invalid_port', ['port'], 'port'],
      [400, 'telephony.invalid_currency', ['currency'], 'price'],
      // Sem `details[]`: o campo sai do código.
      [409, 'telephony.trunk_name_taken', [], 'name'],
      [400, 'telephony.invalid_amount', [], 'price'],
      [422, 'secrets.encryption_unconfigured', [], 'password'],
      // Do pedido inteiro.
      [409, 'telephony.trunk_in_use', [], null],
      [400, 'invalid_argument', [], null],
    ]
    for (const [status, code, fields, campo] of casos) {
      const e = recusa(status, code, fields.map((field) => ({ field, description: code })))
      expect(trunkErrorField(e), code).toBe(campo)
    }
  })

  it('um código conhecido tem chave de tradução; um desconhecido não se inventa', () => {
    expect(errorKey('telephony.trunk_host_refused')).toBe('telecom.erro.trunk_host_refused')
    expect(errorKey('secrets.encryption_unconfigured')).toBe('telecom.erro.encryption_unconfigured')
    expect(errorKey('telephony.codigo_novo')).toBeNull()
    expect(errorKey(null)).toBeNull()
  })

  it('todos os códigos que a consola diz conhecer têm texto nas quatro línguas', async () => {
    const codigos = [
      'srtp_requires_tls', 'trunk_host_refused', 'trunk_name_taken', 'trunk_in_use', 'invalid_trunk_name', 'invalid_short_code',
      'invalid_trunk_host', 'invalid_port', 'invalid_transport', 'invalid_srtp', 'invalid_scope', 'invalid_prefixes',
      'invalid_max_channels', 'invalid_username', 'invalid_password', 'invalid_trunk_order', 'invalid_amount', 'invalid_currency',
      'invalid_rate', 'price_backdated', 'price_exists', 'rate_exists', 'emergency_cannot_be_blocked', 'emergency_must_be_external',
      'emergency_never_recorded', 'emergency_rule_without_emergency_number', 'rule_requires_trunk', 'rule_trunk_not_allowed',
      'fallback_equals_trunk', 'unknown_trunk', 'invalid_pattern', 'invalid_rule_description', 'invalid_rule_action', 'too_many_rules',
      'invalid_number',
    ]
    for (const lang of ['pt', 'en', 'fr', 'zh']) {
      const loc = (await import(`../../locales/${lang}/telecom.ts`)).default as unknown as { erro: Record<string, string> }
      for (const c of codigos) {
        expect(errorKey(`telephony.${c}`), c).toBe(`telecom.erro.${c}`)
        expect(loc.erro[c], `${lang}.erro.${c}`).toBeTruthy()
      }
      expect(loc.erro.encryption_unconfigured, lang).toBeTruthy()
      expect(loc.erro.generico, lang).toBeTruthy()
    }
  })
})

describe('as quatro línguas têm as mesmas chaves de telefonia', () => {
  const chaves = (o: unknown, prefixo = ''): string[] =>
    Object.entries(o as Record<string, unknown>).flatMap(([k, v]) => (v && typeof v === 'object' ? chaves(v, `${prefixo}${k}.`) : [`${prefixo}${k}`]))

  it('en, fr e zh não têm chaves a mais nem a menos do que pt', async () => {
    const pt = chaves((await import('../../locales/pt/telecom')).default).sort()
    // O chinês não tem plural: `_one` só existe nas outras.
    const semPlural = (l: string[]) => l.filter((k) => !k.endsWith('_one'))
    for (const lang of ['en', 'fr']) {
      expect(chaves((await import(`../../locales/${lang}/telecom.ts`)).default).sort(), lang).toEqual(pt)
    }
    const zh = chaves((await import('../../locales/zh/telecom')).default)
    for (const k of semPlural(pt)) expect(zh, k).toContain(k)
    for (const k of zh) expect(pt, k).toContain(k)
  })
})

describe('reordenar', () => {
  it('subir e descer trocam com o vizinho', () => {
    expect(moveItem(['a', 'b', 'c'], 1, -1)).toEqual(['b', 'a', 'c'])
    expect(moveItem(['a', 'b', 'c'], 1, 1)).toEqual(['a', 'c', 'b'])
  })

  it('nas pontas devolve a MESMA lista — não há pedido a fazer', () => {
    const l = ['a', 'b', 'c']
    expect(moveItem(l, 0, -1)).toBe(l)
    expect(moveItem(l, 2, 1)).toBe(l)
    expect(moveItem(l, 7, 1)).toBe(l)
  })

  it('nunca perde nem duplica elementos (a ordem mandada é sempre uma permutação)', () => {
    const l = ['a', 'b', 'c', 'd']
    for (let i = 0; i < l.length; i++) {
      for (const d of [-1, 1] as const) expect([...moveItem(l, i, d)].sort()).toEqual(l)
      for (let j = 0; j < l.length; j++) expect([...moveTo(l, i, j)].sort()).toEqual(l)
    }
  })

  it('arrastar põe o elemento na posição de destino', () => {
    expect(moveTo(['a', 'b', 'c', 'd'], 0, 2)).toEqual(['b', 'c', 'a', 'd'])
    expect(moveTo(['a', 'b', 'c', 'd'], 3, 0)).toEqual(['d', 'a', 'b', 'c'])
    const l = ['a', 'b']
    expect(moveTo(l, 1, 1)).toBe(l)
    expect(moveTo(l, -1, 0)).toBe(l)
  })
})

describe('assistente «Ligar operadora móvel»', () => {
  it('oferece Unitel, Africell, Movicel e «outra», com os códigos curtos', () => {
    expect(OPERATORS.map((o) => [o.id, o.name, o.short_code])).toEqual([
      ['unitel', 'Unitel', 'UNI'],
      ['africell', 'Africell', 'AFR'],
      ['movicel', 'Movicel', 'MOV'],
      ['other', '', ''],
    ])
  })

  it('NENHUM preset traz host, canais, utilizador, password ou preço', () => {
    for (const o of OPERATORS) {
      for (const sec of ['tls_srtp', 'udp_plain'] as const) {
        const f = presetForm(o.id, sec)
        expect(f.host, `${o.id} host`).toBe('')
        expect(f.max_channels, `${o.id} canais`).toBe('')
        expect(f.username, `${o.id} utilizador`).toBe('')
        expect(f.password, `${o.id} password`).toBe('')
        expect(f.price_amount, `${o.id} preço`).toBe('')
        // E por isso não se cria sem os preencher.
        const e = validateTrunkForm(f, 'create')
        expect(e.host?.key).toBe('hostObrigatorio')
        expect(e.max_channels?.key).toBe('canaisObrigatorio')
      }
    }
  })

  it('o módulo dos presets não contém nenhum endereço: nem domínio, nem IP', async () => {
    const { readFileSync } = await import('node:fs')
    const { join } = await import('node:path')
    const src = readFileSync(join(__dirname, 'operatorPresets.ts'), 'utf8')
    expect(src).not.toMatch(/\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b/)
    expect(src).not.toMatch(/[a-z0-9-]+\.(ao|com|net|org|co)\b/i)
    expect(src).not.toMatch(/sip:|sbc\./i)
  })

  it('o habitual: TLS na 5061, SRTP obrigatório, nacional, sem registo, activa', () => {
    for (const o of OPERATORS) {
      const f = presetForm(o.id)
      expect([f.transport, f.port, f.srtp, f.scope, f.register, f.enabled], o.id).toEqual(['tls', '5061', 'mandatory', 'national', false, true])
      expect(f.name).toBe(o.name)
      expect(f.short_code).toBe(o.short_code)
    }
  })

  it('se a operadora só dá UDP sem SRTP, o preset é coerente (passa a regra SRTP↔TLS) e pede rede privada', () => {
    const f = presetForm('movicel', 'udp_plain')
    expect([f.transport, f.srtp, f.port]).toEqual(['udp', 'off', '5060'])
    expect(srtpNeedsTls(f.transport, f.srtp)).toBe(false)
    expect(isCleartext(f.transport, f.srtp)).toBe(true)
  })

  it('os prefixos são sugestões válidas, e «outra operadora» não sugere nenhum', () => {
    for (const o of OPERATORS) {
      for (const p of o.suggestedPrefixes) expect(isPrefix(p), `${o.id} ${p}`).toBe(true)
      expect(presetForm(o.id).prefixes).toBe(o.suggestedPrefixes.join(', '))
    }
    expect(presetForm('other').prefixes).toBe('')
    // Nenhum prefixo sugerido a duas operadoras.
    const todos = OPERATORS.flatMap((o) => o.suggestedPrefixes)
    expect(new Set(todos).size).toBe(todos.length)
  })

  it('cada coisa a pedir à operadora tem texto nas quatro línguas, e a sugestão diz que é sugestão', async () => {
    expect(ASK_OPERATOR).toEqual(['sbc', 'origem', 'autenticacao', 'codecs', 'dtmf', 'numeros', 'identidade', 'canais'])
    for (const lang of ['pt', 'en', 'fr', 'zh']) {
      const loc = (await import(`../../locales/${lang}/telecom.ts`)).default as unknown as { assistente: { pedir: Record<string, string>; prefixosSugestao: string } }
      for (const k of ASK_OPERATOR) expect(loc.assistente.pedir[k], `${lang}.${k}`).toBeTruthy()
      expect(loc.assistente.prefixosSugestao, lang).toBeTruthy()
    }
    const pt = (await import('../../locales/pt/telecom')).default as unknown as { assistente: { pedir: Record<string, string>; prefixosSugestao: string } }
    expect(pt.assistente.prefixosSugestao).toBe('Sugestão — confirmar com a operadora.')
    expect(pt.assistente.pedir.codecs).toContain('G.711 lei A')
    expect(pt.assistente.pedir.dtmf).toContain('RFC 4733')
    expect(pt.assistente.pedir.numeros).toContain('E.164')
    expect(pt.assistente.pedir.identidade).toContain('P-Asserted-Identity')
  })
})
