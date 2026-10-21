import { describe, expect, it } from 'vitest'
import type { StreamDestination } from '../api'
import { accaoDeGuardar, deDestinoGuardado, kindDoUrl, paraOAr, temChave } from './destinosGuardados'

const guardado = (over: Partial<StreamDestination> = {}): StreamDestination => ({
  id: 'd1',
  org_id: 'o1',
  kind: 'youtube',
  label: 'YouTube',
  url: 'rtmp://a.rtmp.youtube.com/live2',
  key_prefix: 'abc',
  has_key: true,
  state: 'ready',
  created_by: 'u1',
  created_at: '2026-10-06T00:00:00Z',
  updated_at: '2026-10-06T00:00:00Z',
  ...over,
})

describe('destinos guardados', () => {
  it('a chave do servidor não volta ao browser, mas o facto de existir volta', () => {
    const d = deDestinoGuardado(guardado())
    expect(d.chave).toBe('')
    expect(d.temChaveGuardada).toBe(true)
    expect(d).toMatchObject({ id: 'd1', url: 'rtmp://a.rtmp.youtube.com/live2', rotulo: 'YouTube' })
    // É isto que estava partido: um destino carregado lia-se como «sem chave»
    // e o botão «ir para o ar» ficava desligado com tudo configurado.
    expect(temChave(d)).toBe(true)
    expect(temChave(deDestinoGuardado(guardado({ has_key: false })))).toBe(false)
    // E uma chave escrita agora conta, como sempre contou.
    expect(temChave({ url: 'rtmp://x', chave: ' k ' })).toBe(true)
    expect(temChave({ url: 'rtmp://x', chave: '   ' })).toBe(false)
  })

  it('o kind sai do URL, e o que não se reconhece é rtmp', () => {
    expect(kindDoUrl('rtmp://a.rtmp.youtube.com/live2')).toBe('youtube')
    expect(kindDoUrl('RTMPS://live-api-s.FACEBOOK.com/rtmp/')).toBe('facebook')
    expect(kindDoUrl('rtmps://1-tcp.linkedin.com/x')).toBe('linkedin')
    expect(kindDoUrl('rtmp://meu-servidor.ao/live')).toBe('rtmp')
    expect(kindDoUrl('  ')).toBe('rtmp')
  })

  it('um cartão sem URL não cria lixo na organização', () => {
    expect(accaoDeGuardar(undefined, { url: '', chave: '' })).toEqual({})
    expect(accaoDeGuardar(undefined, { url: '   ', chave: 'k', rotulo: 'X' })).toEqual({})
  })

  it('sem id e com URL, cria — e leva a chave só nesse momento', () => {
    expect(accaoDeGuardar(undefined, { url: 'rtmp://a.rtmp.youtube.com/live2', chave: 'k', rotulo: 'O meu canal' })).toEqual({
      criar: { kind: 'youtube', label: 'O meu canal', url: 'rtmp://a.rtmp.youtube.com/live2', stream_key: 'k' },
    })
    // Sem rótulo, o nome é o tipo — um destino sem nome nenhum na lista da
    // organização não se distingue dos outros.
    expect(accaoDeGuardar(undefined, { url: 'rtmp://meu.ao/live', chave: '' })).toEqual({
      criar: { kind: 'rtmp', label: 'rtmp', url: 'rtmp://meu.ao/live' },
    })
  })

  it('com id, só vai ao servidor o que mudou', () => {
    const antes = { id: 'd1', url: 'rtmp://a/live', chave: '', rotulo: 'A', temChaveGuardada: true }
    // Nada mudou: nada se envia.
    expect(accaoDeGuardar(antes, { ...antes })).toEqual({})
    // Só o rótulo.
    expect(accaoDeGuardar(antes, { ...antes, rotulo: 'B' })).toEqual({ actualizar: { label: 'B' } })
    // Só o URL.
    expect(accaoDeGuardar(antes, { ...antes, url: 'rtmp://b/live' })).toEqual({
      actualizar: { url: 'rtmp://b/live' },
    })
    // Uma chave nova é uma operação à parte: substituir uma credencial não é
    // editar um nome.
    expect(accaoDeGuardar(antes, { ...antes, chave: 'nova' })).toEqual({ rotarChave: 'nova' })
    // E as duas coisas ao mesmo tempo, que é o que faz quem corrige o URL e
    // cola a chave de uma vez.
    expect(accaoDeGuardar(antes, { ...antes, url: 'rtmp://b/live', chave: 'nova' })).toEqual({
      actualizar: { url: 'rtmp://b/live' },
      rotarChave: 'nova',
    })
  })

  it('um destino guardado VAI AO AR — e é por isso que a regra é uma só', () => {
    const guardados = [deDestinoGuardado(guardado()), deDestinoGuardado(guardado({ id: 'd2', has_key: false }))]
    const alvos = paraOAr(guardados)
    // O que estava partido: o arranque filtrava por `chave.trim()` e a chave de
    // um destino guardado não volta ao browser — com todos guardados, o pedido
    // saía vazio e o servidor recusava com `SemDestino`.
    expect(alvos).toHaveLength(1)
    expect(alvos[0].id).toBe('d1')
    expect(guardados.filter((d) => d.chave.trim())).toHaveLength(0)
    // Controlo negativo: a regra tem de ser EXACTAMENTE a do botão.
    expect(paraOAr(guardados)).toEqual(guardados.filter(temChave))
  })

  it('uma chave escrita agora continua a ir ao ar, e um destino vazio não', () => {
    const escritos = [
      { url: 'rtmp://a/live', chave: 'k' },
      { url: 'rtmp://b/live', chave: '  ' },
    ]
    expect(paraOAr(escritos).map((d) => d.url)).toEqual(['rtmp://a/live'])
  })
})
