/**
 * A sala do Estúdio: a fila de convidados, «Pôr no palco», a pré-escuta, o
 * chat interno, as perguntas e as sondagens.
 *
 * CARREGA-SE POR `lazy()` e só quando a pessoa pede. O Estúdio grava e corta
 * sem rede (é o que o PWA promete), e uma aula gravada sozinha não pode
 * arrastar o caminho de media de uma chamada: os módulos `signaling` e
 * `webrtc` vivem só neste chunk, fora do grafo estático do Estúdio e do
 * precache (ver `studio.invariantes.test.ts`).
 *
 * A página só recebe o que interessa ao palco: quem está NO PALCO (as fontes
 * que o compositor desenha e mistura), a sondagem a mostrar, e a sala e o
 * token com que o directo pode emitir.
 */
import { FormEvent, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, createRoom } from '../../api'
import type { Fonte } from '../../room/compositor'
import { ChatPanel } from '../../room/ChatPanel'
import { PollsPanel } from '../../room/PollsPanel'
import { QaPanel } from '../../room/QaPanel'
import { useChat } from '../../room/useChat'
import { useMeetingTools } from '../../room/useMeetingTools'
import { useParticipants } from '../../room/useParticipants'
import { useRoomCore, type RemotePeer } from '../../room/useRoomCore'
import { parseRoomCode } from '../../roomCode'
import type { SondagemNoPalco } from '../../studio/desenho'
import { Icon } from '../../ui/icons'
import { Alert, Avatar, Button, cx, IconButton, Spinner, Tabs, TextInput } from '../../ui/kit'
import '../../ui/room.css'
import { useLigacaoDoEstudio } from './useLigacaoDoEstudio'

export interface PropsDaSala {
  titulo: string
  micId: string
  obterCamara: () => MediaStreamTrack | null
  /** A sala e o token com que o directo emite; `null` ao desligar. */
  onLigacao: (l: { codigo: string; token: string } | null) => void
  onPalco: (fontes: Fonte[]) => void
  onSondagem: (s: SondagemNoPalco | null) => void
  /** Separador inicial: no telemóvel abre no chat. */
  separadorInicial?: Separador
}

type Separador = 'convidados' | 'chat' | 'perguntas' | 'sondagens'

export default function SalaDoEstudio(props: PropsDaSala) {
  const { t } = useTranslation()
  const [codigo, setCodigo] = useState('')
  const [escrito, setEscrito] = useState('')
  const [aCriar, setACriar] = useState(false)
  const [erro, setErro] = useState('')

  async function criar() {
    setACriar(true)
    setErro('')
    try {
      // Sala de espera LIGADA: é ela que faz a fila — ninguém entra no
      // estúdio sem o anfitrião o deixar.
      const sala = await createRoom(props.titulo.trim() || t('studio.semTitulo'), 'sfu', true, false, 'normal')
      setCodigo(sala.code)
    } catch (e) {
      setErro(apiErrorMessage(e, t('studio.sala.erros.criar')))
    } finally {
      setACriar(false)
    }
  }

  function entrar(e: FormEvent) {
    e.preventDefault()
    const c = parseRoomCode(escrito)
    if (!c) {
      setErro(t('studio.sala.erros.codigo'))
      return
    }
    setErro('')
    setCodigo(c)
  }

  if (codigo) {
    return (
      <SessaoDaSala
        key={codigo}
        {...props}
        codigo={codigo}
        onSair={() => {
          props.onLigacao(null)
          props.onPalco([])
          props.onSondagem(null)
          setCodigo('')
        }}
      />
    )
  }

  return (
    <section className="st-group st-panel" data-studio="sala" aria-labelledby="st-sala-h">
      <h2 id="st-sala-h" className="st-group__title">
        {t('studio.sala.titulo')}
      </h2>
      <p className="st-note">{t('studio.sala.explicacao')}</p>
      <div className="st-actions">
        <Button variant="primary" size="sm" icon="userPlus" busy={aCriar} data-studio="sala-criar" onClick={() => void criar()}>
          {t('studio.sala.criar')}
        </Button>
      </div>
      <form className="st-sala-join" onSubmit={entrar}>
        <TextInput
          value={escrito}
          placeholder={t('studio.sala.codigoPh')}
          aria-label={t('studio.sala.codigo')}
          spellCheck={false}
          onChange={(e) => setEscrito(e.target.value)}
        />
        <Button type="submit" size="sm" variant="secondary" disabled={!escrito.trim()}>
          {t('studio.sala.ligar')}
        </Button>
      </form>
      {erro && <Alert tone="danger">{erro}</Alert>}
    </section>
  )
}

function SessaoDaSala({
  codigo,
  titulo: _titulo,
  micId,
  obterCamara,
  onLigacao,
  onPalco,
  onSondagem,
  onSair,
  separadorInicial = 'convidados',
}: PropsDaSala & { codigo: string; onSair: () => void }) {
  const { t } = useTranslation()
  const core = useRoomCore(codigo, 'connecting')
  const ligacao = useLigacaoDoEstudio(core, { micId, obterCamara })
  const [separador, setSeparador] = useState<Separador>(separadorInicial)
  const pessoas = useParticipants(core, false)
  const chat = useChat(core, separador === 'chat')
  const ferramentas = useMeetingTools(core)
  const [noPalco, setNoPalco] = useState<Set<string>>(() => new Set())
  const [preEscuta, setPreEscuta] = useState<Set<string>>(() => new Set())
  const [escolhido, setEscolhido] = useState('')
  const [copiado, setCopiado] = useState(false)

  const ligada = ligacao.estado === 'ligada'
  const convidados = core.peers.filter((p) => !p.is_bot)

  // Quem saiu da sala sai do palco e da pré-escuta.
  useEffect(() => {
    const ids = new Set(core.peers.map((p) => p.peerId))
    setNoPalco((s) => (([...s].every((id) => ids.has(id)) ? s : new Set([...s].filter((id) => ids.has(id))))))
    setPreEscuta((s) => (([...s].every((id) => ids.has(id)) ? s : new Set([...s].filter((id) => ids.has(id))))))
  }, [core.peers])

  // O palco recebe as fontes pela ORDEM em que foram postas lá.
  const ordem = useRef<string[]>([])
  const fontes = useMemo<Fonte[]>(() => {
    ordem.current = [...ordem.current.filter((id) => noPalco.has(id)), ...[...noPalco].filter((id) => !ordem.current.includes(id))]
    return ordem.current
      .map((id) => core.peers.find((p) => p.peerId === id))
      .filter((p): p is RemotePeer => !!p)
      .map((p) => ({ id: p.peerId, nome: p.username, stream: p.stream }))
  }, [noPalco, core.peers])
  useEffect(() => onPalco(fontes), [fontes, onPalco])

  // O directo emite pela MESMA sala — é nela que o servidor autentica.
  useEffect(() => {
    const token = core.roomTokenRef.current
    onLigacao(ligada && token ? { codigo, token } : null)
  }, [ligada, codigo, core.roomTokenRef, onLigacao])

  // A sondagem mais recente vai para a sobreposição, com os números do servidor.
  const ultima = ferramentas.polls[ferramentas.polls.length - 1]
  useEffect(() => {
    onSondagem(
      ultima
        ? {
            pergunta: ultima.question,
            opcoes: ultima.options.map((o, i) => ({ texto: o, votos: ultima.counts[i] ?? 0 })),
            aberta: ultima.open,
          }
        : null,
    )
  }, [ultima, onSondagem])

  // Ao sair do Estúdio (ou desligar a sala) o palco fica sem convidados.
  useEffect(
    () => () => {
      onPalco([])
      onSondagem(null)
      onLigacao(null)
    },
    [onPalco, onSondagem, onLigacao],
  )

  // SFU: vídeo inteiro de quem está no palco, a camada baixa para as miniaturas.
  useEffect(() => {
    if (!ligada || core.topology !== 'sfu') return
    const quality: Record<string, 'q' | 'h' | 'f'> = {}
    for (const p of convidados) quality[p.peerId] = noPalco.has(p.peerId) ? 'f' : 'q'
    core.signal.send({ type: 'video-interest', peers: convidados.map((p) => p.peerId), quality })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ligada, core.topology, core.peers, noPalco])

  const alternar = (setter: typeof setNoPalco, id: string) =>
    setter((s) => {
      const n = new Set(s)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })

  /** Pôr/tirar do palco avisa o próprio par — é a luz de tally da câmara-telemóvel. */
  const alternarNoPalco = (id: string) => {
    const aoVivo = !noPalco.has(id)
    alternar(setNoPalco, id)
    core.signal.send({ type: 'tally', to: id, live: aoVivo })
  }

  const pessoa = convidados.find((p) => p.peerId === escolhido) ?? null
  const link = `${location.origin}${location.pathname}#/r/${codigo}`
  const perguntasAbertas = ferramentas.questions.filter((q) => !q.answered)
  const destacada = [...perguntasAbertas].sort((a, b) => b.upvotes - a.upvotes)[0]

  return (
    <section className="st-group st-panel st-sala" data-studio="sala" aria-labelledby="st-sala-h">
      <header className="st-group__head">
        <h2 id="st-sala-h" className="st-group__title">
          {t('studio.sala.fila')}
        </h2>
        <span className="dx-spacer" />
        <span className="dx-num dx-muted st-small" data-studio="sala-espera">
          {t('studio.sala.emEspera', { count: pessoas.waitingQueue.length })}
        </span>
      </header>

      <div className="st-sala__bar">
        <code className="dx-num st-sala__code">{codigo}</code>
        <IconButton
          icon={copiado ? 'check' : 'link'}
          bare
          label={t('studio.sala.copiarConvite')}
          onClick={() => {
            void navigator.clipboard?.writeText(link).then(() => {
              setCopiado(true)
              window.setTimeout(() => setCopiado(false), 1500)
            })
          }}
        />
        <span className="dx-spacer" />
        <Button size="sm" variant="ghost" icon="logout" data-studio="sala-sair" onClick={onSair}>
          {t('studio.sala.desligar')}
        </Button>
      </div>

      {ligacao.estado === 'a-ligar' && <Spinner label={t('studio.sala.aLigar')} />}
      {ligacao.estado === 'a-espera' && <Alert tone="warning">{t('studio.sala.aEspera')}</Alert>}
      {ligacao.estado === 'recusada' && <Alert tone="danger">{t('studio.sala.recusada')}</Alert>}
      {(ligacao.estado === 'caiu' || ligacao.estado === 'erro') && (
        <Alert tone="danger">
          {ligacao.erro || t('studio.sala.caiu')}{' '}
          <Button size="sm" variant="secondary" icon="refresh" onClick={ligacao.religar}>
            {t('studio.sala.religar')}
          </Button>
        </Alert>
      )}
      {ligada && ligacao.erro && <Alert tone="warning">{ligacao.erro}</Alert>}

      {ligada && (
        <>
          <Tabs<Separador>
            label={t('studio.sala.separadores')}
            value={separador}
            onChange={setSeparador}
            tabs={[
              { value: 'convidados', label: t('studio.sala.convidados'), count: pessoas.waitingQueue.length },
              { value: 'chat', label: t('studio.sala.chat'), count: separador === 'chat' ? 0 : chat.unread },
              { value: 'perguntas', label: t('studio.sala.perguntas'), count: perguntasAbertas.length },
              { value: 'sondagens', label: t('studio.sala.sondagens') },
            ]}
          />

          {separador === 'convidados' && (
            <div className="st-sala__body">
              {pessoas.waitingQueue.length > 0 && (
                <ul className="st-wait" aria-label={t('studio.sala.aPorta')}>
                  {pessoas.waitingQueue.map((w) => (
                    <li key={w.peer_id} className="st-wait__item" data-studio="sala-espera-item">
                      <Avatar name={w.username} size={24} />
                      <span className="st-ellipsis">{w.username}</span>
                      <span className="dx-spacer" />
                      <Button size="sm" variant="primary" data-studio="sala-admitir" onClick={() => pessoas.admit(w.peer_id, true)}>
                        {t('studio.sala.admitir')}
                      </Button>
                      <IconButton icon="x" bare label={t('studio.sala.recusar', { nome: w.username })} onClick={() => pessoas.admit(w.peer_id, false)} />
                    </li>
                  ))}
                </ul>
              )}

              {convidados.length === 0 ? (
                <p className="st-note">{t('studio.sala.semConvidados')}</p>
              ) : (
                <div className="st-guests" role="listbox" aria-label={t('studio.sala.convidados')}>
                  {convidados.map((p) => (
                    <Convidado
                      key={p.peerId}
                      pessoa={p}
                      escolhido={escolhido === p.peerId}
                      noPalco={noPalco.has(p.peerId)}
                      aOuvir={noPalco.has(p.peerId) || preEscuta.has(p.peerId)}
                      onEscolher={() => setEscolhido(p.peerId)}
                    />
                  ))}
                </div>
              )}

              <div className="st-guests__actions">
                <Button
                  variant={pessoa && noPalco.has(pessoa.peerId) ? 'secondary' : 'primary'}
                  size="sm"
                  disabled={!pessoa}
                  data-studio="por-no-palco"
                  onClick={() => pessoa && alternarNoPalco(pessoa.peerId)}
                >
                  {pessoa && noPalco.has(pessoa.peerId) ? t('studio.sala.tirarDoPalco') : t('studio.sala.porNoPalco')}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  icon="volume"
                  disabled={!pessoa || noPalco.has(pessoa.peerId)}
                  aria-pressed={!!pessoa && preEscuta.has(pessoa.peerId)}
                  data-studio="pre-escuta"
                  onClick={() => pessoa && alternar(setPreEscuta, pessoa.peerId)}
                >
                  {t('studio.sala.preEscuta')}
                </Button>
              </div>
              <p className="st-note">{t('studio.sala.notaPreEscuta')}</p>
            </div>
          )}

          {separador === 'chat' && (
            <div className="st-sala__body st-sala__chat">
              {destacada && (
                <div className="st-pinned" data-studio="pergunta-destacada">
                  <span className="st-pinned__head dx-num">{t('studio.sala.perguntaVotos', { count: destacada.upvotes })}</span>
                  <span>{destacada.text}</span>
                </div>
              )}
              <ChatPanel chat={chat} isHost={core.isHost} code={codigo} peers={core.peers} tools={ferramentas} onNewPoll={() => setSeparador('sondagens')} />
            </div>
          )}
          {separador === 'perguntas' && (
            <div className="st-sala__body">
              <QaPanel tools={ferramentas} isHost={core.isHost} />
            </div>
          )}
          {separador === 'sondagens' && (
            <div className="st-sala__body">
              <PollsPanel tools={ferramentas} isHost={core.isHost} present={core.peers.length + 1} />
            </div>
          )}
        </>
      )}

      {/* O som dos convidados: ouvido só por quem está no estúdio, e só de quem
          está no palco ou em pré-escuta. O `<audio>` existe para todos porque o
          Chromium não entrega áudio remoto ao Web Audio (a mistura) sem um
          elemento de media agarrado à track. */}
      <div hidden>
        {convidados.map((p) =>
          p.stream ? <SomDoConvidado key={p.peerId} stream={p.stream} mudo={!(noPalco.has(p.peerId) || preEscuta.has(p.peerId))} /> : null,
        )}
      </div>
    </section>
  )
}

function Convidado({
  pessoa,
  escolhido,
  noPalco,
  aOuvir,
  onEscolher,
}: {
  pessoa: RemotePeer
  escolhido: boolean
  noPalco: boolean
  aOuvir: boolean
  onEscolher: () => void
}) {
  const { t } = useTranslation()
  const video = useRef<HTMLVideoElement>(null)
  useEffect(() => {
    const v = video.current
    if (!v) return
    v.srcObject = pessoa.stream
    if (pessoa.stream) void v.play().catch(() => {})
  }, [pessoa.stream])
  const temVideo = !!pessoa.stream?.getVideoTracks().length && pessoa.camOn
  return (
    <button
      type="button"
      role="option"
      aria-selected={escolhido}
      className={cx('st-guest', escolhido && 'is-selected', noPalco && 'is-live')}
      data-studio="convidado"
      onClick={onEscolher}
    >
      <video ref={video} className="st-guest__video" muted playsInline hidden={!temVideo} />
      {!temVideo && (
        <span className="st-guest__avatar" aria-hidden="true">
          <Avatar name={pessoa.username} size={34} />
        </span>
      )}
      <span className="st-guest__name">
        {aOuvir && <Icon name="volume" size={11} />}
        {pessoa.username}
      </span>
      {noPalco && <span className="st-guest__badge dx-num">{t('studio.sala.noPalco')}</span>}
    </button>
  )
}

function SomDoConvidado({ stream, mudo }: { stream: MediaStream; mudo: boolean }) {
  const ref = useRef<HTMLAudioElement>(null)
  useEffect(() => {
    const a = ref.current
    if (!a) return
    a.srcObject = stream
    void a.play().catch(() => {})
  }, [stream])
  return <audio ref={ref} autoPlay muted={mudo} />
}
