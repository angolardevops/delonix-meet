/**
 * «Destinos simultâneos» (ADR-0003): um cartão por destino, na forma do
 * template — etiqueta, nome, canal, estado.
 *
 * O QUE O ESTADO QUER DIZER: a FORMA do cartão (bloqueado, editável) vem do
 * estado GLOBAL do directo — uma só ligação ao servidor, que reparte para
 * todos (ver `destinosLocais.ts`). A tentativa/motivo por baixo, quando o
 * destino individual está em erro, vêm do `porDestino` que o servidor manda
 * a cada 2 s (`directo.ts`, `aoMudarDestinos`) — casado pelo índice `dest`.
 * Kbps e perdas ficam de fora do cartão de propósito: são números que só
 * fazem sentido no ecrã do directo a decorrer, não aqui.
 *
 * Os campos (rótulo, servidor, chave) editam-se num diálogo, para o cartão
 * manter a forma do template.
 */
import { ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert, Button, cx, Dialog, Field, IconButton, TextInput } from '../ui/kit'
import { temChave as temChaveDoDestino } from './destinosGuardados'
import { contagemDosDestinos, estadoDoCartao, ORDEM_DA_CONTAGEM } from './destinosLocais'
import type { EstadoDoCartao } from './destinosLocais'
import { plataformaDoUrl } from './palco'
import type { Destino, EstadoDoDestino, EstadoDoDirecto, Rede } from './directo'

const CHAVE_DO_ESTADO: Record<EstadoDoCartao, string> = {
  'sem-chave': 'semChave',
  pronto: 'pronto',
  'a-ligar': 'aLigar',
  'no-ar': 'noAr',
  erro: 'erro',
}

function CampoChave({ value, onChange, id }: { value: string; onChange: (v: string) => void; id: string }) {
  const { t } = useTranslation()
  const [ver, setVer] = useState(false)
  return (
    <div className="st-secret">
      {/* `type=password`: a chave é uma credencial, e quem configura o directo
          com o ecrã partilhado mostrava-a a toda a gente. */}
      <TextInput
        id={id}
        type={ver ? 'text' : 'password'}
        value={value}
        autoComplete="off"
        spellCheck={false}
        placeholder={t('studio.directo.chavePh')}
        data-studio="destino-chave"
        onChange={(e) => onChange(e.target.value)}
      />
      <IconButton
        icon="eye"
        label={ver ? t('studio.directo.esconderChave') : t('studio.directo.mostrarChave')}
        aria-pressed={ver}
        onClick={() => setVer((v) => !v)}
      />
    </div>
  )
}

/** Editar um destino: os campos num diálogo, aplicados ao «Guardar». */
function DialogoDoDestino({
  d,
  nome,
  podeRemover,
  onFechar,
  onGuardar,
  onRemover,
}: {
  d: Destino
  nome: string
  podeRemover: boolean
  onFechar: () => void
  onGuardar: (d: Destino) => void
  onRemover: () => void
}) {
  const { t } = useTranslation()
  const [rascunho, setRascunho] = useState<Destino>(d)
  const mudar = (patch: Partial<Destino>) => setRascunho((r) => ({ ...r, ...patch }))
  return (
    <Dialog
      title={t('studio.directo.editar', { rotulo: nome })}
      onClose={onFechar}
      footer={
        <>
          {podeRemover && (
            <Button variant="ghost" icon="trash" data-studio="destino-remover" onClick={onRemover}>
              {t('studio.directo.removerCurto')}
            </Button>
          )}
          <span className="dx-spacer" />
          <Button variant="ghost" onClick={onFechar}>
            {t('studio.directo.cancelar')}
          </Button>
          <Button variant="primary" data-studio="destino-guardar" onClick={() => onGuardar(rascunho)}>
            {t('studio.directo.guardar')}
          </Button>
        </>
      }
    >
      <div className="st-dest-dialog" data-studio="destino-form">
        <Field label={t('studio.directo.rotulo')} htmlFor="st-dest-rotulo">
          <TextInput
            id="st-dest-rotulo"
            value={rascunho.rotulo ?? ''}
            autoComplete="off"
            placeholder={t('studio.directo.rotuloPh')}
            onChange={(e) => mudar({ rotulo: e.target.value })}
          />
        </Field>
        <Field label={t('studio.directo.url')} htmlFor="st-dest-url">
          <TextInput
            id="st-dest-url"
            value={rascunho.url}
            autoComplete="off"
            spellCheck={false}
            placeholder="rtmp://"
            data-studio="destino-url"
            onChange={(e) => mudar({ url: e.target.value })}
          />
        </Field>
        <Field label={t('studio.directo.chave')} htmlFor="st-dest-chave">
          <CampoChave id="st-dest-chave" value={rascunho.chave} onChange={(v) => mudar({ chave: v })} />
        </Field>
        <p className="st-note">{t('studio.directo.soNestaSessao')}</p>
      </div>
    </Dialog>
  )
}

function CartaoDoDestino({
  d,
  i,
  fase,
  bloqueado,
  live,
  onEditar,
}: {
  d: Destino
  i: number
  fase: EstadoDoDirecto['fase']
  bloqueado: boolean
  /** O último estado que o SERVIDOR mandou para este destino, casado por índice. */
  live?: EstadoDoDestino
  onEditar: () => void
}) {
  const { t } = useTranslation()
  const nome = d.rotulo?.trim() || t('studio.directo.destino', { n: i + 1 })
  const temChave = temChaveDoDestino(d)
  const estado = estadoDoCartao(fase, temChave)
  return (
    <li className={cx('st-dest', `st-dest--${estado}`)} data-studio="destino" data-estado={estado}>
      <div className="st-dest__head">
        {/* A etiqueta lê-se do HOST do servidor RTMP, não do rótulo. */}
        <span className="st-dest__tag dx-num" data-studio="destino-tag" title={t('studio.directo.tagDica')}>
          {plataformaDoUrl(d.url, location.host)}
        </span>
        <span className="st-dest__who">
          <strong className="st-dest__name">{nome}</strong>
          <span className="st-dest__url dx-num">{d.url.trim() || 'rtmp://'}</span>
        </span>
        <span className="st-dest__state dx-num" data-studio="destino-estado">
          {t(`studio.directo.estados.${CHAVE_DO_ESTADO[estado]}`)}
        </span>
      </div>
      {live?.estado === 'erro' && (
        <div className="st-dest__retry dx-num" data-studio="destino-retry">
          {t('studio.directo.tentativa', { n: live.tentativas })}
          {live.motivo ? ` · ${live.motivo}` : ''}
        </div>
      )}
      <div className="st-dest__foot dx-num">
        <span>{temChave ? t('studio.directo.chaveDefinida') : t('studio.directo.chaveEmFalta')}</span>
        <button
          type="button"
          className="st-dest__edit"
          disabled={bloqueado}
          data-studio="destino-editar"
          aria-label={t('studio.directo.editar', { rotulo: nome })}
          onClick={onEditar}
        >
          {t('studio.directo.editarCurto')}
        </button>
      </div>
    </li>
  )
}

/**
 * A coluna direita do DelonixStudio: «Destinos simultâneos», a gravação local
 * (`children`) e, no fundo, «Adicionar destino» e «Parar emissão».
 */
/**
 * As ESTATÍSTICAS da emissão a decorrer — o que o OBS põe num painel próprio e
 * o estúdio não mostrava em lado nenhum: o que este browser envia, o que está
 * à espera de sair, e o que o servidor diz de cada destino.
 *
 * Só aparece no ar. Fora dele não há nada verdadeiro para mostrar, e um painel
 * com zeros parece avariado.
 */
function Estatisticas({
  rede,
  kbps,
  porDestino,
  destinos,
}: {
  rede: Rede
  kbps: number
  porDestino: EstadoDoDestino[]
  destinos: Destino[]
}) {
  const { t, i18n } = useTranslation()
  const num = (v: number, casas = 0) => v.toLocaleString(i18n.language, { maximumFractionDigits: casas })
  return (
    <section className="st-stats" data-studio="estatisticas" aria-label={t('studio.directo.stats.titulo')}>
      <div className="st-stats__linha dx-num">
        <span>{t('studio.directo.stats.enviado', { kbps: num(kbps) })}</span>
        <span data-studio="debito">
          {rede.adaptativo
            ? t('studio.directo.stats.debito', { mbps: num(rede.debito / 1_000_000, 1) })
            : t('studio.directo.stats.pedido', { mbps: num(rede.alvo / 1_000_000, 1) })}
        </span>
        <span data-studio="fila">{t('studio.directo.stats.fila', { s: num(rede.segundos, 1) })}</span>
        {rede.descartados > 0 && <span className="st-stats__mau">{t('studio.directo.stats.descartados', { n: num(rede.descartados) })}</span>}
      </div>
      {rede.saude !== 'ok' && (
        <div
          className={cx('dx-alert', rede.saude === 'a-acumular' ? 'dx-alert--danger' : 'dx-alert--warning')}
          role="status"
          data-studio="rede-apertada"
          data-saude={rede.saude}
        >
          <span>
            <strong>{t(`studio.directo.stats.${rede.saude}`)}</strong> —{' '}
            {rede.adaptativo ? t('studio.directo.stats.aAdaptar') : t('studio.directo.stats.conselho')}
          </span>
        </div>
      )}
      {porDestino.length > 0 && (
        <ul className="st-stats__dests dx-num">
          {porDestino.map((d) => (
            <li key={d.dest}>
              <span className="st-stats__nome">{d.rotulo || destinos[d.dest]?.rotulo || d.platform}</span>
              <span>{t('studio.directo.stats.destKbps', { kbps: num(d.kbps) })}</span>
              {d.perdas > 0 && <span className="st-stats__mau">{t('studio.directo.stats.perdas', { pct: num(d.perdas, 1) })}</span>}
              {d.framesDescartados > 0 && <span className="st-stats__mau">{t('studio.directo.stats.frames', { n: num(d.framesDescartados) })}</span>}
              {d.tentativas > 0 && <span className="st-stats__mau">{t('studio.directo.stats.tentativas', { count: d.tentativas })}</span>}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

export default function LivePanel({
  suportado,
  destinos,
  porDestino,
  rede,
  kbps,
  localAGravar,
  maximo,
  estado,
  podeEmitir,
  onMudar,
  onAdicionar,
  onRemover,
  onIrParaOAr,
  onParar,
  children,
}: {
  suportado: boolean
  destinos: Destino[]
  /** Último estado por destino que o servidor mandou (`directo.aoMudarDestinos`). */
  porDestino: EstadoDoDestino[]
  /** A saúde do envio deste browser (`directo.aoMudarRede`). */
  rede: Rede
  /** Débito que este browser está a ENVIAR, medido pelos bytes (`useDebito`). */
  kbps: number
  /** A gravação local está mesmo a decorrer — só então a nota de recuperação faz sentido. */
  localAGravar: boolean
  maximo: number
  estado: EstadoDoDirecto
  /** Há imagem para emitir (ecrã ou câmara). */
  podeEmitir: boolean
  onMudar: (i: number, d: Destino) => void
  /** Acrescenta um destino vazio e devolve o índice dele (para o abrir). */
  onAdicionar: () => number
  onRemover: (i: number) => void
  onIrParaOAr: () => void
  onParar: () => void
  /** A gravação local, que no template vive por cima dos botões do fundo. */
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const [aEditar, setAEditar] = useState<number | null>(null)
  const noAr = estado.fase === 'no-ar'
  const aLigar = estado.fase === 'a-ligar'
  const bloqueado = noAr || aLigar
  const n = contagemDosDestinos(destinos, estado.fase)
  const comChave = destinos.length - n['sem-chave']
  const contagem = ORDEM_DA_CONTAGEM.filter((e) => n[e] > 0)
    .map((e) => t(`studio.directo.contagem.${CHAVE_DO_ESTADO[e]}`, { count: n[e] }))
    .join(' · ')
  const emEdicao = aEditar !== null ? destinos[aEditar] : undefined
  // Modo de recuperação (andaime «delonix-meet-recovery»): pelo menos um
  // destino a repetir sozinho, com a gravação local mesmo a decorrer — nunca
  // só porque a emissão nem chegou a arrancar (aLigar também bate no `erro`
  // de um cartão que ainda não tem chave, e isso não é uma queda de rede).
  const emRecuperacao = noAr && localAGravar && porDestino.some((d) => d.estado === 'erro')

  return (
    <section className={cx('st-live', noAr && 'st-live--on')} data-studio="directo" aria-labelledby="st-live-h">
      <header className="st-live__head">
        <h2 id="st-live-h" className="st-live__title">
          {t('studio.directo.titulo')}
        </h2>
        {suportado && (
          <span className="st-live__count dx-num" data-studio="destinos-contagem">
            {contagem}
          </span>
        )}
      </header>

      <div className="st-live__scroll">
        {!suportado ? (
          <Alert tone="warning">{t('studio.directo.indisponivel')}</Alert>
        ) : (
          <>
            <ul className="st-dests">
              {destinos.map((d, i) => (
                <CartaoDoDestino
                  key={i}
                  d={d}
                  i={i}
                  fase={estado.fase}
                  bloqueado={bloqueado}
                  live={porDestino.find((p) => p.dest === i)}
                  onEditar={() => setAEditar(i)}
                />
              ))}
            </ul>
            {/* Marca para os e2e: a emissão foi aceite. */}
            {noAr && <span data-studio="no-ar" hidden />}
            {noAr && <Estatisticas rede={rede} kbps={kbps} porDestino={porDestino} destinos={destinos} />}
            {emRecuperacao && (
              <div className="dx-alert dx-alert--warning" role="status" data-studio="modo-recuperacao">
                <span>
                  <strong>{t('studio.directo.recuperacao.titulo')}</strong> — {t('studio.directo.recuperacao.texto')}
                </span>
              </div>
            )}
            {estado.fase === 'erro' && (
              <div className="dx-alert dx-alert--danger" role="alert" data-studio="directo-erro">
                {estado.motivo}
              </div>
            )}
          </>
        )}
      </div>

      {children}

      {suportado && (
        <div className="st-live__actions">
          <button
            type="button"
            className="st-live__btn"
            disabled={bloqueado || destinos.length >= maximo}
            // O tecto vai no título do botão: uma nota por baixo dos cartões
            // não existe no template e empurrava a gravação local.
            title={destinos.length >= maximo ? t('studio.directo.limite', { maximo }) : undefined}
            data-studio="destino-adicionar"
            onClick={() => setAEditar(onAdicionar())}
          >
            {t('studio.directo.adicionar')}
          </button>
          {noAr || aLigar ? (
            <button type="button" className="st-live__btn st-live__btn--stop" data-studio="sair-do-ar" disabled={aLigar} onClick={onParar}>
              {aLigar ? t('studio.directo.aLigar') : t('studio.directo.parar')}
            </button>
          ) : (
            <button
              type="button"
              className="st-live__btn st-live__btn--go"
              disabled={comChave === 0 || !podeEmitir}
              data-studio="ir-para-o-ar"
              onClick={onIrParaOAr}
            >
              {t('studio.directo.irParaOAr')}
            </button>
          )}
        </div>
      )}

      {emEdicao && aEditar !== null && (
        <DialogoDoDestino
          d={emEdicao}
          nome={emEdicao.rotulo?.trim() || t('studio.directo.destino', { n: aEditar + 1 })}
          podeRemover={destinos.length > 1}
          onFechar={() => setAEditar(null)}
          onGuardar={(d) => {
            onMudar(aEditar, d)
            setAEditar(null)
          }}
          onRemover={() => {
            onRemover(aEditar)
            setAEditar(null)
          }}
        />
      )}
    </section>
  )
}
