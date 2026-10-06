/**
 * A tabela de endereços. É o controlo negativo da auditoria de navegação de
 * 2026-10-06, virado em teste: cada linha marcada «ERA» é um caso que abria o
 * Início sem o dizer.
 */
import { describe, expect, it } from 'vitest'
import { parseHash } from './rota'

describe('parseHash', () => {
  it('a sala tolera maiúsculas e parâmetros a mais', () => {
    // ERA: `#/r/ABC-DEFG-HIJ` → home. Um cliente de correio que normalize
    // maiúsculas deixava o convidado no Início, com a sala na barra.
    expect(parseHash('#/r/ABC-DEFG-HIJ')).toEqual({
      kind: 'room',
      code: 'abc-defg-hij',
      voice: false,
    })
    // ERA: qualquer parâmetro a mais → home. Um encurtador acrescenta-os.
    expect(parseHash('#/r/abc-defg-hij?utm_source=email')).toEqual({
      kind: 'room',
      code: 'abc-defg-hij',
      voice: false,
    })
    // `?voice` já funcionava; `?voice=1` ERA home.
    for (const h of ['#/r/sala?voice', '#/r/sala?voice=1', '#/r/sala?voice=true']) {
      expect(parseHash(h)).toEqual({ kind: 'room', code: 'sala', voice: true })
    }
    // E dizer que NÃO se quer voz é respeitado.
    for (const h of ['#/r/sala?voice=0', '#/r/sala?voice=false']) {
      expect(parseHash(h)).toEqual({ kind: 'room', code: 'sala', voice: false })
    }
    // O parâmetro a mais não ganha ao `voice`.
    expect(parseHash('#/r/sala?utm=x&voice')).toEqual({
      kind: 'room',
      code: 'sala',
      voice: true,
    })
  })

  it('a sala de espera e o telemóvel seguem a mesma regra da sala', () => {
    expect(parseHash('#/lobby/ABC-def')).toEqual({ kind: 'lobby', code: 'abc-def' })
    expect(parseHash('#/lobby/abc-def?x=1')).toEqual({ kind: 'lobby', code: 'abc-def' })
    expect(parseHash('#/telemovel/ABC-def')).toEqual({ kind: 'telemovel', code: 'abc-def' })
  })

  it('os tokens NÃO se normalizam — normalizar um token é perdê-lo', () => {
    expect(parseHash('#/invite/AbC_123-xyz')).toEqual({ kind: 'invite', token: 'AbC_123-xyz' })
    expect(parseHash('#/share/deadbeef00')).toEqual({ kind: 'share', token: 'deadbeef00' })
    // Um token de partilha é hexadecimal: o que não o é não é uma partilha.
    expect(parseHash('#/share/NAOHEX')).toEqual({ kind: 'desconhecida', endereco: '#/share/NAOHEX' })
  })

  it('um nome de página casa por segmento, não por prefixo', () => {
    expect(parseHash('#/admin')).toEqual({ kind: 'admin' })
    expect(parseHash('#/admin?org=1')).toEqual({ kind: 'admin' })
    expect(parseHash('#/admin/membros')).toEqual({ kind: 'admin' })
    // ERA: estes três abriam a página do prefixo.
    expect(parseHash('#/adminzinho')).toEqual({ kind: 'desconhecida', endereco: '#/adminzinho' })
    expect(parseHash('#/airplane')).toEqual({ kind: 'desconhecida', endereco: '#/airplane' })
    expect(parseHash('#/roomsx')).toEqual({ kind: 'desconhecida', endereco: '#/roomsx' })
  })

  it('o leitor de gravações pede um uuid, e o resto é a biblioteca', () => {
    const uuid = '3f2b1c4d-5e6f-7a8b-9c0d-ddddeeeeffff'
    expect(parseHash(`#/recordings/${uuid}`)).toEqual({ kind: 'player', id: uuid })
    expect(parseHash(`#/recordings/${uuid}?t=42`)).toEqual({ kind: 'player', id: uuid })
    // Um id que não é uuid cai na biblioteca (por segmento), como antes.
    expect(parseHash('#/recordings/nao-e-uuid')).toEqual({ kind: 'recordings' })
    expect(parseHash('#/recordings')).toEqual({ kind: 'recordings' })
  })

  it('o diagrama aceita com e sem id', () => {
    expect(parseHash('#/whiteboards/diagram')).toEqual({ kind: 'diagram', id: null })
    expect(parseHash('#/whiteboards/diagram/abc_1')).toEqual({ kind: 'diagram', id: 'abc_1' })
    expect(parseHash('#/whiteboards/diagram?sala=x')).toEqual({ kind: 'diagram', id: null })
    expect(parseHash('#/whiteboards')).toEqual({ kind: 'whiteboards' })
  })

  it('um hash que não é caminho é o Início — e isso inclui as âncoras da página', () => {
    // O `#conteudo` do «saltar para o conteúdo» não é uma rota. Passou a ser um
    // botão (não uma âncora), mas o parser não pode depender disso.
    for (const h of ['', '#', '#/', '#conteudo', '#termos', '#privacidade']) {
      expect(parseHash(h)).toEqual({ kind: 'home' })
    }
  })

  it('o que não é rota nenhuma diz-se, e carrega o endereço consigo', () => {
    // ERA: tudo isto mostrava o Início com a barra a dizer outra coisa.
    for (const h of ['#/estudio', '#/settings', '#/qualquer-coisa', '#/r/', '#/lobby/']) {
      expect(parseHash(h)).toEqual({ kind: 'desconhecida', endereco: h })
    }
  })
})
