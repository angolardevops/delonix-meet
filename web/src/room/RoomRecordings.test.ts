/**
 * As gravações da sala (painel das pessoas) desenhadas para HTML, com linhas
 * na forma que `GET /api/rooms/{code}/recordings` devolve. Não substitui um
 * browser (não prova layout nem o clique): prova o que o painel OFERECE e o
 * que DIZ por estado — nenhum botão sobre uma gravação sem ficheiro nem sobre
 * uma que quem vê não pode descarregar (R59), «A processar N%» e a causa da
 * falha em vez de «0.0 MB», e nenhuma chave de tradução crua, nas quatro línguas.
 */
import { createElement as h } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import type { RecordingLibraryItem } from '../api'

// O cliente da API lê a sessão do armazenamento ao carregar: em Node não existe.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })

const { default: i18n } = await import('../i18n')
const { RoomRecordings } = await import('./RoomRecordings')

// `setLanguage` mexe no `document`; aqui carregam-se os dicionários à mão.
const DICTS = {
  'pt-AO': null,
  en: () => import('../locales/en'),
  'fr-FR': () => import('../locales/fr'),
  'zh-CN': () => import('../locales/zh'),
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

const rec = (over: Partial<RecordingLibraryItem> = {}): RecordingLibraryItem => ({
  id: 'x',
  room_id: 'r',
  uploader_id: 'u',
  filename: 'Reunião abc.webm',
  size_bytes: 3 * 1_048_576,
  created_at: '2026-10-05T10:00:00Z',
  room_code: 'abc',
  uploader_name: 'Ana',
  owned: true,
  share_count: 0,
  can_download: true,
  status: 'ready',
  failure_reason: null,
  state: 'ready',
  progress_pct: null,
  kind: 'meeting',
  duration_ms: null,
  width: null,
  height: null,
  fps: null,
  video_codec: null,
  audio_codec: null,
  has_thumbnail: false,
  transcript_status: 'none',
  transcript_language: null,
  transcribed_at: null,
  chapter_count: 0,
  comment_count: 0,
  view_count: 0,
  participant_count: 0,
  caption_languages: [],
  description: '',
  tags: [],
  visibility: 'private',
  published_at: null,
  can_manage: true,
  uploader_org_id: null,
  uploader_org_name: null,
  ...over,
})

// A linha como o gravador a insere quando a gravação pára: 0 bytes, a compor.
const aCompor = (pct: number | null = 42) => rec({ id: 'c', status: 'processing', state: 'processing', progress_pct: pct, size_bytes: 0 })
const falhada = (reason: string | null = 'O disco encheu a meio da composição.') =>
  rec({ id: 'f', status: 'failed', state: 'failed', failure_reason: reason, size_bytes: 0 })

const render = (recordings: RecordingLibraryItem[]) => renderToStaticMarkup(h(RoomRecordings, { recordings, onDownload: () => {} }))
const botoes = (html: string) => (html.match(/<button/g) ?? []).length
const semChavesCruas = (html: string) => expect(html).not.toMatch(/(room|recordings|ui)\.[a-zA-Z]/)

describe('gravações da sala — o que o painel oferece por estado', () => {
  it('a compor: sem botão, «A processar N%», e nunca «0.0 MB»', async () => {
    await usar('pt-AO')
    const html = render([aCompor(42)])
    expect(botoes(html)).toBe(0)
    expect(html).toContain('Reunião abc.webm')
    expect(html).toContain('A processar 42%')
    expect(html).not.toContain('MB')
    expect(html).not.toContain('Falhada')
  })

  it('a compor sem percentagem ainda: «A processar», sem número inventado', async () => {
    await usar('pt-AO')
    const html = render([aCompor(null)])
    expect(botoes(html)).toBe(0)
    expect(html).toContain('A processar')
    expect(html).not.toContain('%')
  })

  it('falhada: sem botão, e diz a causa que o servidor registou', async () => {
    await usar('pt-AO')
    const html = render([falhada()])
    expect(botoes(html)).toBe(0)
    expect(html).toContain('Falhada')
    expect(html).toContain('O disco encheu a meio da composição.')
    expect(html).not.toContain('MB')
  })

  it('falhada sem causa registada: diz que não ficou ficheiro', async () => {
    await usar('pt-AO')
    const html = render([falhada(null)])
    expect(botoes(html)).toBe(0)
    expect(html).toContain('A gravação falhou e não ficou ficheiro.')
  })

  it('um estado que a consola não conhece falha fechado: sem botão, lê-se como falhada', async () => {
    await usar('pt-AO')
    const html = render([rec({ status: 'archiving' as RecordingLibraryItem['status'] })])
    expect(botoes(html)).toBe(0)
    expect(html).toContain('A gravação falhou e não ficou ficheiro.')
    expect(html).not.toContain('MB')
  })

  it('pronta e com permissão: o botão de descarregar, com o tamanho', async () => {
    await usar('pt-AO')
    const html = render([rec()])
    expect(botoes(html)).toBe(1)
    expect(html).toMatch(/3\sMiB/)
    // O nome acessível do botão diz o que ele faz, não só o nome do ficheiro.
    expect(html).toMatch(/<button[^>]*>.*Descarregar.*Reunião abc\.webm/)
    expect(html).not.toContain('Só quem gravou')
  })

  it('a transcrever tem ficheiro: descarrega-se como uma pronta', async () => {
    await usar('pt-AO')
    expect(botoes(render([rec({ status: 'transcribing', state: 'transcribing' })]))).toBe(1)
  })

  it('pronta mas quem vê não a pode descarregar: aparece, sem botão (o `?dl=1` daria 403)', async () => {
    await usar('pt-AO')
    const html = render([rec({ can_download: false, owned: true })])
    expect(botoes(html)).toBe(0)
    expect(html).toContain('Reunião abc.webm')
    expect(html).toMatch(/3\sMiB/)
    // …e diz porquê: sem isto era um cartão igual ao botão, que não respondia.
    expect(html).toContain('Só quem gravou, ou um administrador da organização, a pode descarregar.')
    expect(html).not.toContain('Descarregar')
  })

  it('numa lista misturada, só as descarregáveis têm botão', async () => {
    await usar('pt-AO')
    const html = render([aCompor(), falhada(), rec({ id: 'a' }), rec({ id: 'b', can_download: false }), rec({ id: 'd', status: 'transcribing' })])
    expect(botoes(html)).toBe(2)
  })

  it('é uma lista, com uma região de estado para anunciar o fim da composição', async () => {
    await usar('pt-AO')
    const html = render([aCompor(), falhada(), rec({ id: 'a' })])
    expect((html.match(/<ul/g) ?? []).length).toBe(1)
    expect((html.match(/<li/g) ?? []).length).toBe(3)
    expect(html).toContain('role="status"')
  })

  it('falhada sem causa não repete «Falhada»: a frase de recurso já o diz', async () => {
    await usar('pt-AO')
    expect(render([falhada(null)])).not.toContain('Falhada ·')
  })

  it('sem gravações: diz que não há', async () => {
    await usar('pt-AO')
    expect(render([])).toContain('Ainda não há gravações.')
  })

  it('nenhuma chave de tradução crua, nas quatro línguas', async () => {
    for (const lng of Object.keys(DICTS) as Lng[]) {
      await usar(lng)
      const html = render([aCompor(42), aCompor(null), falhada(), falhada(null), rec(), rec({ can_download: false })])
      semChavesCruas(html)
      expect(botoes(html)).toBe(1)
      // Uma chave em falta cai no português sem erro nenhum (`fallbackLng`):
      // «sem chave crua» não chega para dizer que está traduzido.
      if (lng !== 'pt-AO') {
        for (const pt of ['A processar', 'Falhada', 'A gravação falhou', 'Só quem gravou', 'Descarregar']) expect(html, `${lng}: «${pt}»`).not.toContain(pt)
      }
    }
    await usar('pt-AO')
  })
})
