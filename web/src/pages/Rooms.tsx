/**
 * Salas — a lista das salas de que sou dono, com criar, alterar e apagar.
 *
 * Porque existe: criavam-se salas do Início, da agenda e da paleta, e depois
 * **não havia onde as ver**. Uma sala é um endereço permanente que se mete
 * numa assinatura e num convite; sem lista, o código que se criou há duas
 * semanas só se reencontrava a abrir o histórico do browser.
 *
 * O que a API sustenta, e só isso:
 * - GET /api/rooms — as MINHAS salas, paginadas por cursor. Não é «as salas
 *   da organização»: a tabela não tem org_id (ver o comentário do handler).
 * - POST /api/rooms — nome, topologia, sala de espera, E2EE, formato.
 * - PATCH /api/rooms/{code} — hoje só a entrada de convidados.
 * - DELETE /api/rooms/{code} — recusa com 409 se a sala tiver gravações, se
 *   estiver marcada numa reunião futura, ou se for a pessoal. A razão chega
 *   no code do erro e é ela que se mostra, não um texto nosso.
 *
 * Renomear não aparece porque o servidor não o faz: o PATCH só aceita
 * allow_guests. Um campo que o cliente escreve e o servidor ignora é pior do
 * que um que não existe.
 */
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ApiError,
  apiErrorMessage,
  createRoom,
  deleteRoom,
  listMyRooms,
  patchRoom,
  type Room,
  type RoomPage,
} from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Icon } from '../ui/icons'
import { Alert, Button, Checkbox, cx, Dialog, Empty, Field, Select, StatusBadge, TextInput, Toggle } from '../ui/kit'
import { Menu, useMenuDeContexto, type AccaoDeMenu, type EventoDePonteiro } from '../ui/Menu'
import '../ui/salas.css'
import { copiarTexto } from '../ui/copy'

type Formato = 'normal' | 'training'

/**
 * As três recusas do `DELETE`, traduzidas pelo `code`, que é contrato estável
 * (`delonix-meet-api` §7). A mensagem do servidor é escrita para quem lê um
 * log — está sempre em português e numa consola em inglês fica a meio do
 * caminho. Um `code` que não conheçamos cai na mensagem dele, que é sempre
 * melhor do que «algo correu mal».
 */
function razaoDaRecusa(e: unknown, t: (k: string) => string): string | null {
  const code = e instanceof ApiError ? (e.body as { code?: string } | null)?.code : undefined
  switch (code) {
    case 'room.has_recordings':
      return t('salas.recusa.gravacoes')
    case 'room.has_scheduled_meeting':
      return t('salas.recusa.agenda')
    case 'room.personal_cannot_be_deleted':
      return t('salas.recusa.pessoal')
    default:
      return null
  }
}

/** O endereço completo de uma sala — é isto que se cola num convite. */
function enderecoDaSala(code: string): string {
  return `${location.origin}/#/r/${code}`
}

export default function Rooms() {
  const { t, i18n } = useTranslation()
  const { enterRoom } = useShell()
  const [paginas, setPaginas] = useState<Room[]>([])
  const [cursor, setCursor] = useState<string | undefined>()
  const [aPedirMais, setAPedirMais] = useState(false)
  const [criar, setCriar] = useState(false)
  const [apagar, setApagar] = useState<Room | null>(null)
  const [copiada, setCopiada] = useState<string | null>(null)
  const [erro, setErro] = useState('')

  const primeira = useAsync<RoomPage>((signal) => listMyRooms(signal), [])
  const reload = primeira.reload
  const recomecar = useCallback(() => {
    setPaginas([])
    setCursor(undefined)
    reload()
  }, [reload])

  /** A primeira página vem do `useAsync`; as seguintes acumulam-se aqui. */
  const todas = (d: RoomPage) => [...d.items, ...paginas]
  const proximo = (d: RoomPage) => (paginas.length ? cursor : d.next_page_token)

  const mais = useCallback(
    async (token: string) => {
      setAPedirMais(true)
      setErro('')
      try {
        const p = await listMyRooms(undefined, token)
        setPaginas((v) => [...v, ...p.items])
        setCursor(p.next_page_token)
      } catch (e) {
        setErro(apiErrorMessage(e, t('salas.erroCarregar')))
      } finally {
        setAPedirMais(false)
      }
    },
    [t],
  )

  const copiar = useCallback(async (code: string) => {
    try {
      if (!(await copiarTexto(enderecoDaSala(code)))) return
      setCopiada(code)
      setTimeout(() => setCopiada((c) => (c === code ? null : c)), 2000)
    } catch {
      /* sem permissão de área de transferência: o endereço continua visível na linha */
    }
  }, [])

  const alternarConvidados = useCallback(
    async (sala: Room) => {
      setErro('')
      try {
        await patchRoom(sala.code, !sala.allow_guests)
        recomecar()
      } catch (e) {
        setErro(apiErrorMessage(e, t('salas.erroGuardar')))
      }
    },
    [recomecar, t],
  )

  return (
    <div className="page sl">
      <PageBar title={t('salas.titulo')} meta={primeira.state.s === 'ready' ? t('salas.contagem', { count: todas(primeira.state.d).length }) : undefined}>
        <Button variant="primary" icon="plus" onClick={() => setCriar(true)} data-salas="nova">
          {t('salas.nova')}
        </Button>
      </PageBar>

      {erro && <Alert tone="danger">{erro}</Alert>}

      <AsyncSection state={primeira.state} onRetry={recomecar}>
        {(d) => {
          const salas = todas(d)
          const token = proximo(d)
          if (salas.length === 0) {
            return (
              <Empty
                icon="door"
                title={t('salas.vazioTitulo')}
                action={
                  <Button variant="primary" icon="plus" onClick={() => setCriar(true)}>
                    {t('salas.nova')}
                  </Button>
                }
              >
                {t('salas.vazioTexto')}
              </Empty>
            )
          }
          return (
            <>
              <Tabela
                salas={salas}
                copiada={copiada}
                lingua={i18n.language}
                onEntrar={(s) => enterRoom(s.code)}
                onCopiar={copiar}
                onConvidados={alternarConvidados}
                onApagar={setApagar}
              />
              {token && (
                <div className="sl-mais">
                  <Button variant="outline" busy={aPedirMais} onClick={() => void mais(token)}>
                    {t('salas.verMais')}
                  </Button>
                </div>
              )}
            </>
          )
        }}
      </AsyncSection>

      {criar && (
        <DialogoDeCriacao
          onFechar={() => setCriar(false)}
          onCriada={(sala) => {
            setCriar(false)
            recomecar()
            void copiar(sala.code)
          }}
        />
      )}
      {apagar && <DialogoDeApagar sala={apagar} onFechar={() => setApagar(null)} onApagada={() => (setApagar(null), recomecar())} />}
    </div>
  )
}

// ------------------------------------------------------------------ tabela

function Tabela({
  salas,
  copiada,
  lingua,
  onEntrar,
  onCopiar,
  onConvidados,
  onApagar,
}: {
  salas: Room[]
  copiada: string | null
  lingua: string
  onEntrar: (s: Room) => void
  onCopiar: (code: string) => void
  onConvidados: (s: Room) => void
  onApagar: (s: Room) => void
}) {
  const { t } = useTranslation()
  const menu = useMenuDeContexto()
  const [alvo, setAlvo] = useState<Room | null>(null)

  const aoContexto = (s: Room) => (e: EventoDePonteiro) => {
    setAlvo(s)
    menu.abrir(e)
  }
  const accoes: AccaoDeMenu[] = alvo
    ? [
        { id: 'entrar', label: t('salas.entrar'), icon: 'door', onPick: () => onEntrar(alvo) },
        { id: 'copiar', label: t('salas.copiarLink'), icon: 'link', onPick: () => onCopiar(alvo.code) },
        { id: 'convidados', label: t('salas.convidados'), icon: 'people', marcado: !!alvo.allow_guests, onPick: () => onConvidados(alvo) },
        { id: 'apagar', label: t('salas.apagar'), icon: 'trash', perigo: true, onPick: () => onApagar(alvo) },
      ]
    : []

  return (
    <div className="dx-table-wrap">
      <Menu ponto={menu.ponto} accoes={accoes} label={t('salas.accoesDe', { nome: alvo?.name ?? '' })} onFechar={menu.fechar}>
        {alvo?.name}
      </Menu>
      <table className="dx-table sl-table">
        <caption className="dx-sr-only">{t('salas.titulo')}</caption>
        <thead>
          <tr>
            <th scope="col">{t('salas.coluna.sala')}</th>
            <th scope="col">{t('salas.coluna.codigo')}</th>
            <th scope="col">{t('salas.coluna.politica')}</th>
            <th scope="col">{t('salas.coluna.criada')}</th>
            <th scope="col">
              <span className="dx-sr-only">{t('salas.coluna.accoes')}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {salas.map((s) => (
            <tr key={s.id} className="sl-row" data-sala={s.code} onContextMenu={aoContexto(s)}>
              <td>
                <strong className="sl-row__nome">{s.name}</strong>
                {s.format === 'training' && <span className="sl-row__sub">{t('salas.formato.training')}</span>}
              </td>
              <td>
                <button
                  type="button"
                  className={cx('sl-code dx-num', copiada === s.code && 'is-copiada')}
                  title={t('salas.copiarLink')}
                  onClick={() => onCopiar(s.code)}
                >
                  <Icon name={copiada === s.code ? 'check' : 'link'} size={12} />
                  {s.code}
                </button>
              </td>
              <td className="sl-politica">
                {s.waiting_room && <StatusBadge tone="neutral" icon="door">{t('salas.salaDeEspera')}</StatusBadge>}
                {s.e2ee && <StatusBadge tone="success" icon="lock">{t('salas.e2ee')}</StatusBadge>}
                {s.allow_guests && <StatusBadge tone="neutral" icon="people">{t('salas.convidadosCurto')}</StatusBadge>}
              </td>
              <td className="dx-num dx-muted">{s.created_at ? new Date(s.created_at).toLocaleDateString(lingua) : '—'}</td>
              <td className="sl-accoes">
                <Button size="sm" variant="secondary" onClick={() => onEntrar(s)}>
                  {t('salas.entrar')}
                </Button>
                <Button size="sm" variant="ghost" icon="trash" onClick={() => onApagar(s)} data-salas="apagar">
                  <span className="dx-sr-only">{t('salas.apagarSala', { nome: s.name })}</span>
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ------------------------------------------------------------------ criar

function DialogoDeCriacao({ onFechar, onCriada }: { onFechar: () => void; onCriada: (s: Room) => void }) {
  const { t } = useTranslation()
  const [nome, setNome] = useState('')
  const [formato, setFormato] = useState<Formato>('normal')
  const [salaDeEspera, setSalaDeEspera] = useState(false)
  const [e2ee, setE2ee] = useState(false)
  const [aCriar, setACriar] = useState(false)
  const [erro, setErro] = useState('')

  async function criar() {
    const limpo = nome.trim()
    if (!limpo) return
    setACriar(true)
    setErro('')
    try {
      onCriada(await createRoom(limpo, 'sfu', salaDeEspera, e2ee, formato))
    } catch (e) {
      setErro(apiErrorMessage(e, t('salas.erroCriar')))
    } finally {
      setACriar(false)
    }
  }

  return (
    <Dialog
      title={t('salas.nova')}
      onClose={onFechar}
      footer={
        <>
          <Button onClick={onFechar}>{t('ui.cancelar')}</Button>
          <Button variant="primary" busy={aCriar} disabled={!nome.trim()} onClick={() => void criar()} data-salas="criar">
            {t('salas.criar')}
          </Button>
        </>
      }
    >
      {erro && <Alert tone="danger">{erro}</Alert>}
      <Field label={t('salas.nome')} htmlFor="sl-nome">
        <TextInput id="sl-nome" value={nome} maxLength={100} onChange={(e) => setNome(e.target.value)} />
      </Field>
      <Field label={t('salas.formato.rotulo')} htmlFor="sl-formato" hint={t('salas.formato.dica')}>
        <Select id="sl-formato" value={formato} onChange={(e) => setFormato(e.target.value as Formato)}>
          <option value="normal">{t('salas.formato.normal')}</option>
          <option value="training">{t('salas.formato.training')}</option>
        </Select>
      </Field>
      <Checkbox label={t('salas.salaDeEspera')} checked={salaDeEspera} onChange={(e) => setSalaDeEspera(e.target.checked)} />
      {/* O `Checkbox` do kit não tem `hint` (o `Toggle` tem): a dica fica
          encostada por baixo, alinhada com o rótulo. */}
      <p className="sl-hint dx-muted">{t('salas.salaDeEsperaDica')}</p>
      <Toggle label={t('salas.e2ee')} hint={t('salas.e2eeDica')} checked={e2ee} onChange={(e) => setE2ee(e.target.checked)} />
    </Dialog>
  )
}

// ------------------------------------------------------------------ apagar

function DialogoDeApagar({ sala, onFechar, onApagada }: { sala: Room; onFechar: () => void; onApagada: () => void }) {
  const { t } = useTranslation()
  const [aApagar, setAApagar] = useState(false)
  const [erro, setErro] = useState('')

  async function confirmar() {
    setAApagar(true)
    setErro('')
    try {
      await deleteRoom(sala.code)
      onApagada()
    } catch (e) {
      // O servidor recusa com uma razão identificada pelo `code`; o texto dele
      // é para um log, não para um ecrã. Um código desconhecido cai na
      // mensagem do servidor — inventar aqui um texto seria adivinhar.
      setErro(razaoDaRecusa(e, t) ?? apiErrorMessage(e, t('salas.erroApagar')))
    } finally {
      setAApagar(false)
    }
  }

  return (
    <Dialog
      title={t('salas.apagarSala', { nome: sala.name })}
      onClose={onFechar}
      footer={
        <>
          <Button onClick={onFechar}>{t('ui.cancelar')}</Button>
          <Button variant="danger" busy={aApagar} onClick={() => void confirmar()} data-salas="confirmar-apagar">
            {t('salas.apagar')}
          </Button>
        </>
      }
    >
      {erro ? <Alert tone="danger">{erro}</Alert> : <p>{t('salas.apagarAviso')}</p>}
      <p className="dx-muted">{t('salas.apagarGuarda')}</p>
    </Dialog>
  )
}
