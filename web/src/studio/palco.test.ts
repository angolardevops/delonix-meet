/**
 * A matemática do palco do Estúdio: layouts, qualidade, plataforma do URL,
 * sobreposições saneadas e formatos. Sem canvas nem DOM — o desenho real
 * prova-se por pixéis em `e2e/estudio.mjs`.
 */
import { describe, expect, it } from 'vitest'
import { partirEmLinhas } from './desenho'
import {
  formatarBytes,
  hhmmss,
  LARGURA_DO_DESTAQUE,
  LAYOUTS,
  MAXIMO_POR_LAYOUT,
  plataformaDoUrl,
  QUALIDADES,
  rectsDoLayout,
  rotuloDaQualidade,
  sanearSobreposicoes,
  SOBREPOSICOES_INICIAIS,
  eh4k,
  ganhoValido,
  lerQualidade,
} from './palco'

const area = (rs: { w: number; h: number }[]) => rs.reduce((n, r) => n + r.w * r.h, 0)

describe('rectsDoLayout — cada layout cobre o palco inteiro, sem buracos', () => {
  for (const layout of LAYOUTS) {
    it(`${layout}: a área somada é a do canvas, para 1–9 fontes`, () => {
      for (let n = 1; n <= 9; n++) {
        const rs = rectsDoLayout(layout, n, 1920, 1080)
        expect(rs.length).toBe(Math.min(n, MAXIMO_POR_LAYOUT[layout]))
        expect(Math.round(area(rs))).toBe(1920 * 1080)
      }
    })
  }

  it('sem fontes não há nada a desenhar', () => {
    for (const layout of LAYOUTS) expect(rectsDoLayout(layout, 0, 1920, 1080)).toEqual([])
  })

  it('solo mostra UMA fonte a ecrã inteiro, mesmo com mais', () => {
    expect(rectsDoLayout('solo', 3, 1920, 1080)).toEqual([{ x: 0, y: 0, w: 1920, h: 1080 }])
  })

  it('lado-a-lado são duas metades', () => {
    expect(rectsDoLayout('lado-a-lado', 4, 1920, 1080)).toEqual([
      { x: 0, y: 0, w: 960, h: 1080 },
      { x: 960, y: 0, w: 960, h: 1080 },
    ])
  })

  it('destaque: a principal ocupa 70% da largura e a coluna divide a altura', () => {
    const rs = rectsDoLayout('destaque', 3, 1920, 1080)
    expect(rs[0]).toEqual({ x: 0, y: 0, w: Math.round(1920 * LARGURA_DO_DESTAQUE), h: 1080 })
    expect(rs[1].h).toBe(540)
    expect(rs[2].y).toBe(540)
    expect(rs[1].x).toBe(rs[0].w)
  })

  it('destaque com uma fonte só é ecrã inteiro, não um buraco à direita', () => {
    expect(rectsDoLayout('destaque', 1, 1920, 1080)).toEqual([{ x: 0, y: 0, w: 1920, h: 1080 }])
  })
})

describe('qualidade', () => {
  it('há 4K e 50 fps, com o canvas do tamanho certo', () => {
    expect(QUALIDADES['2160p50']).toMatchObject({ largura: 3840, altura: 2160, fps: 50 })
    expect(rotuloDaQualidade('2160p50')).toBe('2160p · 50 fps')
    expect(eh4k('2160p30')).toBe(true)
    expect(eh4k('1080p50')).toBe(false)
  })

  it('a de omissão continua a ser 1080p a 30 (o e2e e o invariante contam com ela)', () => {
    expect(lerQualidade()).toBe('1080p30')
  })

  it('mais pixéis ou mais fps nunca baixam o débito', () => {
    expect(QUALIDADES['1080p50'].bitrate).toBeGreaterThan(QUALIDADES['1080p30'].bitrate)
    expect(QUALIDADES['2160p30'].bitrate).toBeGreaterThan(QUALIDADES['1080p50'].bitrate)
    expect(QUALIDADES['2160p50'].bitrate).toBeGreaterThan(QUALIDADES['2160p30'].bitrate)
  })
})

describe('plataformaDoUrl — lê o HOST, não o rótulo', () => {
  it.each([
    ['rtmp://a.rtmp.youtube.com/live2', 'YT'],
    ['rtmps://live-api-s.facebook.com:443/rtmp/', 'FB'],
    ['rtmps://abc.channel.linkedin.com/live', 'LI'],
    ['rtmp://live.twitch.tv/app', 'TW'],
    ['rtmp://ingest.global-contribute.live-video.net/app', 'TW'],
    ['rtmp://media.delonix.local/live', 'SRV'],
    ['rtmp://192.168.1.20/live', 'SRV'],
    ['rtmp://10.0.0.5:1935/live', 'SRV'],
    ['rtmp://localhost/live', 'SRV'],
    ['rtmp://parceiro.ao/ch2', 'RTMP'],
    ['', 'RTMP'],
    ['youtube.com sem esquema', 'RTMP'],
  ])('%s → %s', (url, esperado) => {
    expect(plataformaDoUrl(url)).toBe(esperado)
  })

  it('um host que só CONTÉM o nome não engana (nada de youtube.com.evil.io)', () => {
    expect(plataformaDoUrl('rtmp://youtube.com.evil.io/live')).toBe('RTMP')
    expect(plataformaDoUrl('rtmp://notyoutube.com/live')).toBe('RTMP')
  })

  it('o host da própria app é um servidor nosso', () => {
    expect(plataformaDoUrl('rtmp://meet.exemplo.ao/live', 'meet.exemplo.ao:443')).toBe('SRV')
  })

  it('credenciais no URL não entram no host', () => {
    expect(plataformaDoUrl('rtmp://user:pw@a.rtmp.youtube.com/live2')).toBe('YT')
  })
})

describe('sobreposições saneadas', () => {
  it('JSON vazio ou lixo dá o estado inicial', () => {
    expect(sanearSobreposicoes(null)).toEqual(SOBREPOSICOES_INICIAIS)
    expect(sanearSobreposicoes({ rodape: 'sim', minutos: 'x' })).toEqual(SOBREPOSICOES_INICIAIS)
  })

  it('textos cortados e minutos limitados', () => {
    const s = sanearSobreposicoes({ nome: 'x'.repeat(500), minutos: 99999, rodape: true })
    expect(s.nome.length).toBe(60)
    expect(s.minutos).toBe(600)
    expect(s.rodape).toBe(true)
  })

  it('o ganho nunca sai de 0–1,5', () => {
    expect(ganhoValido(9, 1)).toBe(1.5)
    expect(ganhoValido(-2, 1)).toBe(0)
    expect(ganhoValido('0.5', 1)).toBe(1)
  })
})

describe('formatos', () => {
  it('hhmmss', () => {
    expect(hhmmss(0)).toBe('00:00')
    expect(hhmmss(65)).toBe('01:05')
    expect(hhmmss(4064)).toBe('01:07:44')
  })

  it('formatarBytes', () => {
    expect(formatarBytes(512, 'en-GB')).toBe('512 B')
    expect(formatarBytes(1536, 'en-GB')).toBe('1.50 KB')
    expect(formatarBytes(42.8 * 1024 ** 3, 'en-GB')).toBe('42.8 GB')
  })

  it('partirEmLinhas não perde palavras', () => {
    const medir = (t: string) => t.length
    const linhas = partirEmLinhas('a portabilidade de numeração em mais dois quatro quatro', medir, 20)
    expect(linhas.join(' ')).toBe('a portabilidade de numeração em mais dois quatro quatro')
    expect(linhas.every((l) => l.length <= 20 || !l.includes(' '))).toBe(true)
  })
})
