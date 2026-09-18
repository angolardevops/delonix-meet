import { describe, expect, it } from 'vitest'
import { parseCsvLines, parseInviteCsv } from './csv'

describe('parseCsvLines', () => {
  it('separa campos simples por vírgula e ignora linhas em branco', () => {
    expect(parseCsvLines('a,b,c\n\nd,e,f\n')).toEqual([
      ['a', 'b', 'c'],
      ['d', 'e', 'f'],
    ])
  })

  it('respeita aspas com vírgulas lá dentro e aspas escapadas a dobrar', () => {
    expect(parseCsvLines('"Lda, Kaeso","diz ""olá"""')).toEqual([['Lda, Kaeso', 'diz "olá"']])
  })

  it('aceita CRLF e CR sozinho como fim de linha', () => {
    expect(parseCsvLines('a,b\r\nc,d\re,f')).toEqual([
      ['a', 'b'],
      ['c', 'd'],
      ['e', 'f'],
    ])
  })
})

describe('parseInviteCsv', () => {
  it('lê email/title/role/branch pelo header, em qualquer ordem e maiúsculas', () => {
    const csv = 'Branch,Email,Role\nLuanda,ana@empresa.co,admin\n,rui@empresa.co,'
    expect(parseInviteCsv(csv)).toEqual({
      rows: [
        { email: 'ana@empresa.co', title: undefined, role: 'admin', branch: 'Luanda' },
        { email: 'rui@empresa.co', title: undefined, role: undefined, branch: undefined },
      ],
    })
  })

  it('ignora colunas desconhecidas e linhas sem email', () => {
    const csv = 'email,extra\nana@empresa.co,x\n,y'
    expect(parseInviteCsv(csv)).toEqual({
      rows: [{ email: 'ana@empresa.co', title: undefined, role: undefined, branch: undefined }],
    })
  })

  it('sem coluna email devolve o erro em vez de adivinhar', () => {
    expect(parseInviteCsv('nome,cargo\nAna,Gestora').errorKey).toBe('org.csv.erroSemEmail')
  })

  it('texto vazio devolve o erro de vazio', () => {
    expect(parseInviteCsv('').errorKey).toBe('org.csv.erroVazio')
  })
})
