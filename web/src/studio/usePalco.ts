/**
 * O estado do PALCO do Estúdio — qualidade, layout, conteúdo, sobreposições,
 * mistura de áudio, microfone, música, marca e banco de cenas — e a ponte
 * para o compositor imperativo.
 *
 * Porque é um hook e não mais estado na página: a página já é dona da
 * gravação, do directo, do arquivo e da edição. O palco é outra conversa, e
 * misturá-la ali fazia um componente de mil linhas onde cada `useEffect`
 * parece depender de todos os outros.
 *
 * A regra é a mesma do resto do Estúdio: o React guarda a escolha, o
 * compositor LÊ-A a cada frame. Nada disto re-renderiza por causa do vídeo.
 */
import { MutableRefObject, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getAppName, isMarcaDeOrigem } from '../branding'
import { listDevices } from '../media'
import * as banco from './cenas'
import { AVATAR_INICIAL, CompositorDeAula, EstadoDoAvatar } from './compositor'
import {
  ConteudoDoPalco,
  guardarLayout,
  guardarMicrofone,
  guardarMistura,
  guardarQualidade,
  guardarSobreposicoes,
  LayoutDoPalco,
  lerLayout,
  lerMicrofone,
  lerMistura,
  lerQualidade,
  lerSobreposicoes,
  Mistura,
  Qualidade,
  QUALIDADES,
  SOBREPOSICOES_INICIAIS,
  Sobreposicoes,
} from './palco'

export interface Microfone {
  id: string
  nome: string
}

export function usePalco({
  compRef,
  pronto,
  bloqueado,
  titulo,
  organizacao,
  avatar,
  aplicarAvatar,
}: {
  compRef: MutableRefObject<CompositorDeAula | null>
  /** O compositor já existe. */
  pronto: boolean
  /** A gravar ou no ar: a qualidade não pode mudar por baixo do fluxo. */
  bloqueado: boolean
  titulo: string
  organizacao: string
  avatar: EstadoDoAvatar
  aplicarAvatar: (a: EstadoDoAvatar) => void
}) {
  const { t } = useTranslation()
  const [qualidade, setQualidade] = useState<Qualidade>(lerQualidade)
  const [layout, setLayoutState] = useState<LayoutDoPalco>(lerLayout)
  const [conteudo, setConteudo] = useState<ConteudoDoPalco>('fontes')
  const [sobreposicoes, setSobreposicoes] = useState<Sobreposicoes>(lerSobreposicoes)
  const [mistura, setMisturaState] = useState<Mistura>(lerMistura)
  const [microfones, setMicrofones] = useState<Microfone[]>([])
  const [microfone, setMicrofoneState] = useState(lerMicrofone)
  const [musica, setMusica] = useState<{ nome: string } | null>(null)
  const [musicaATocar, setMusicaATocar] = useState(false)
  const [cenas, setCenas] = useState<banco.CenaGuardada[]>([])
  const [cenaActiva, setCenaActiva] = useState('')
  const [bancoIndisponivel, setBancoIndisponivel] = useState(false)
  const [temLogotipo, setTemLogotipo] = useState(false)
  const [logotipo, setLogotipo] = useState<ImageBitmap | null>(null)

  // ---- o compositor lê estes campos a cada frame — basta escrevê-los.
  useEffect(() => {
    const c = compRef.current
    if (!c || !pronto) return
    // Mudar a qualidade a meio é recusado pelo próprio compositor; aqui só se
    // tenta quando não há consumidores, e o valor mostrado segue o real.
    c.definirQualidade(QUALIDADES[qualidade])
  }, [compRef, pronto, qualidade])
  useEffect(() => {
    if (compRef.current) compRef.current.layout = layout
  }, [compRef, pronto, layout])
  useEffect(() => {
    if (compRef.current) compRef.current.conteudo = conteudo
  }, [compRef, pronto, conteudo])
  useEffect(() => {
    if (compRef.current) compRef.current.sobreposicoes = sobreposicoes
  }, [compRef, pronto, sobreposicoes])
  useEffect(() => {
    compRef.current?.definirMistura(mistura)
  }, [compRef, pronto, mistura])
  useEffect(() => {
    const c = compRef.current
    if (!c) return
    c.marca = {
      nome: organizacao || getAppName(),
      titulo: titulo.trim(),
      aviso: t('studio.palco.intervalo'),
      deOrigem: isMarcaDeOrigem(),
      logo: logotipo,
    }
  }, [compRef, pronto, organizacao, titulo, logotipo, t])
  useEffect(() => {
    const c = compRef.current
    if (!c) return
    c.aoMudarMusica = setMusicaATocar
    c.microfoneId = microfone
    return () => {
      c.aoMudarMusica = null
    }
  }, [compRef, pronto, microfone])

  // ---- microfones: a lista actualiza-se quando se liga ou desliga um.
  useEffect(() => {
    let vivo = true
    const ler = () =>
      listDevices()
        .then((d) => {
          if (!vivo) return
          setMicrofones(d.mics.map((m, i) => ({ id: m.deviceId, nome: m.label || t('studio.audio.microfoneN', { n: i + 1 }) })))
        })
        .catch(() => {})
    void ler()
    navigator.mediaDevices?.addEventListener?.('devicechange', ler)
    return () => {
      vivo = false
      navigator.mediaDevices?.removeEventListener?.('devicechange', ler)
    }
  }, [t])

  // ---- banco de cenas: carrega, e semeia as de partida na primeira vez.
  const recarregarCenas = useCallback(() => {
    banco
      .listarCenas()
      .then((cs) => {
        setCenas(cs)
        setBancoIndisponivel(false)
      })
      .catch(() => setBancoIndisponivel(true))
  }, [])

  useEffect(() => {
    const base = { avatar: { ...AVATAR_INICIAL }, sobreposicoes: { ...SOBREPOSICOES_INICIAIS }, miniatura: null }
    banco
      .semearSeVazio([
        { ...base, nome: t('studio.cenas.iniciais.oradora'), layout: 'solo', conteudo: 'fontes' },
        { ...base, nome: t('studio.cenas.iniciais.painel'), layout: 'grelha', conteudo: 'fontes' },
        { ...base, nome: t('studio.cenas.iniciais.intervalo'), layout: 'solo', conteudo: 'marca' },
        { ...base, nome: t('studio.cenas.iniciais.quadro'), layout: 'solo', conteudo: 'quadro' },
      ])
      .catch(() => false)
      .finally(recarregarCenas)
    banco
      .lerLogotipo()
      .then(async (b) => {
        setTemLogotipo(!!b)
        setLogotipo(b ? await createImageBitmap(b).catch(() => null) : null)
      })
      .catch(() => {})
    // Semeia-se uma vez por montagem; os nomes ficam na língua em que se semeou.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recarregarCenas])

  // ---- acções

  const escolherQualidade = useCallback(
    (q: Qualidade) => {
      if (bloqueado) return
      setQualidade(q)
      guardarQualidade(q)
    },
    [bloqueado],
  )

  const escolherLayout = useCallback((l: LayoutDoPalco) => {
    setLayoutState(l)
    guardarLayout(l)
    setCenaActiva('')
  }, [])

  const escolherConteudo = useCallback((c: ConteudoDoPalco) => {
    setConteudo(c)
    setCenaActiva('')
  }, [])

  const mudarSobreposicoes = useCallback(
    (patch: Partial<Sobreposicoes>) => {
      setSobreposicoes((s) => {
        const novo = { ...s, ...patch }
        const c = compRef.current
        // A animação de entrada do rodapé e o cronómetro contam desde que LIGAM.
        if (c && patch.rodape && !s.rodape) c.rodapeDesde = Date.now()
        if (c && ((patch.cronometro && !s.cronometro) || (patch.minutos !== undefined && patch.minutos !== s.minutos)))
          c.cronometroDesde = Date.now()
        guardarSobreposicoes(novo)
        return novo
      })
    },
    [compRef],
  )

  const mudarMistura = useCallback((patch: Partial<Mistura>) => {
    setMisturaState((m) => {
      const novo = { ...m, ...patch }
      guardarMistura(novo)
      return novo
    })
  }, [])

  const escolherMicrofone = useCallback(
    (id: string) => {
      setMicrofoneState(id)
      guardarMicrofone(id)
      void compRef.current?.trocarMicrofone(id)
    },
    [compRef],
  )

  const carregarMusica = useCallback(
    (f: File | null) => {
      compRef.current?.definirMusica(f)
      setMusica(f ? { nome: f.name } : null)
      setMusicaATocar(false)
    },
    [compRef],
  )

  const alternarMusica = useCallback(() => {
    const c = compRef.current
    if (!c) return
    if (c.musicaATocar) c.pararMusica()
    else void c.tocarMusica()
  }, [compRef])

  const carregarLogotipo = useCallback(async (f: File | null) => {
    await banco.guardarLogotipo(f).catch(() => undefined)
    setTemLogotipo(!!f)
    setLogotipo(f ? await createImageBitmap(f).catch(() => null) : null)
  }, [])

  const aplicarCena = useCallback(
    (cena: banco.CenaGuardada) => {
      setLayoutState(cena.layout)
      guardarLayout(cena.layout)
      setConteudo(cena.conteudo)
      aplicarAvatar({ ...avatar, ...cena.avatar, modo: avatar.modo })
      mudarSobreposicoes({
        ...cena.sobreposicoes,
        // Os textos são da sessão (quem fala hoje); a cena só decide o que aparece.
        nome: sobreposicoes.nome,
        cargo: sobreposicoes.cargo,
        url: sobreposicoes.url,
        minutos: sobreposicoes.minutos,
      })
      setCenaActiva(cena.id)
    },
    [aplicarAvatar, avatar, mudarSobreposicoes, sobreposicoes],
  )

  const novaCena = useCallback(
    async (nome: string, conteudoDaCena: ConteudoDoPalco) => {
      const c = compRef.current
      // A cena nova já mostra o que vai guardar: o compositor muda JÁ e a
      // miniatura espera dois frames para sair com o conteúdo certo.
      setConteudo(conteudoDaCena)
      if (c) {
        c.conteudo = conteudoDaCena
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
      }
      const miniatura = c ? await banco.miniaturaDe(c.canvas).catch(() => null) : null
      const cena = await banco.guardarCena({ nome, layout, conteudo: conteudoDaCena, avatar, sobreposicoes, miniatura })
      recarregarCenas()
      setCenaActiva(cena.id)
    },
    [avatar, compRef, layout, recarregarCenas, sobreposicoes],
  )

  const apagarCena = useCallback(
    async (id: string) => {
      await banco.apagarCena(id).catch(() => undefined)
      if (cenaActiva === id) setCenaActiva('')
      recarregarCenas()
    },
    [cenaActiva, recarregarCenas],
  )

  return {
    qualidade,
    escolherQualidade,
    layout,
    escolherLayout,
    conteudo,
    escolherConteudo,
    sobreposicoes,
    mudarSobreposicoes,
    mistura,
    mudarMistura,
    microfones,
    microfone,
    escolherMicrofone,
    musica,
    musicaATocar,
    carregarMusica,
    alternarMusica,
    temLogotipo,
    carregarLogotipo,
    cenas,
    cenaActiva,
    bancoIndisponivel,
    aplicarCena,
    novaCena,
    apagarCena,
  }
}

export type Palco = ReturnType<typeof usePalco>
