/**
 * O que os cinco ecrãs do estúdio de TV partilham e que depende do Estúdio:
 * o contexto que a página lhes passa, os selos de REC e AO VIVO, os nomes
 * das fontes e o cartão de uma fonte no barramento.
 */
import type { MutableRefObject } from 'react'
import { useTranslation } from 'react-i18next'
import type { Fonte } from '../../../room/compositor'
import Cronometro from '../../../studio/Cronometro'
import type { CompositorDeAula } from '../../../studio/compositor'
import type { Destino, EstadoDoDirecto } from '../../../studio/directo'
import type { FonteDaMesa } from '../../../studio/tv/fontes'
import { tallyDe } from '../../../studio/tv/mesa'
import type { Palco } from '../../../studio/usePalco'
import { cx } from '../../../ui/kit'
import { Miniatura } from './pecas'
import type { EcraTv, SessaoTv } from './useSessaoTv'

export interface ContextoDoEstudio {
  compRef: MutableRefObject<CompositorDeAula | null>
  /** O nó do palco do Estúdio, para onde o canvas do programa volta. */
  canvasHostRef: MutableRefObject<HTMLDivElement | null>
  palco: Palco
  titulo: string
  temEcra: boolean
  participantes: Fonte[]
  haSondagem: boolean
  gravacao: { estado: 'parado' | 'a-gravar' | 'pausa'; lerSegundos: () => number; e4k: boolean }
  directo: EstadoDoDirecto
  destinos: Destino[]
  kbps: number
  onPararGravacao: () => Promise<void> | void
  onTerminarEmissao: () => Promise<void> | void
  /** `null` volta à emissão do Estúdio. */
  onNavegar: (ecra: EcraTv | null) => void
}

export interface PropsDoEcra {
  s: SessaoTv
  c: ContextoDoEstudio
}

/** Relógio, REC e AO VIVO do topo — só aparecem quando são verdade. */
export function SelosDoAr({ c, destinosNoSelo = true }: { c: ContextoDoEstudio; destinosNoSelo?: boolean }) {
  const { t } = useTranslation()
  const gravando = c.gravacao.estado !== 'parado'
  const noAr = c.directo.fase === 'no-ar'
  const desde = c.directo.fase === 'no-ar' ? c.directo.desde : 0
  const comChave = c.destinos.filter((d) => d.chave.trim()).length
  return (
    <>
      {(gravando || noAr) && (
        <span className="tv-chip tv-relogio dx-num" data-tv="relogio">
          {gravando ? (
            <Cronometro activo={c.gravacao.estado === 'a-gravar'} ler={c.gravacao.lerSegundos} label={t('tv.topo.tempoDeGravacao')} />
          ) : (
            <Cronometro activo ler={() => (desde ? (Date.now() - desde) / 1000 : 0)} label={t('tv.topo.tempoNoAr')} />
          )}
        </span>
      )}
      {gravando && (
        <span className="tv-selo tv-selo--rec" data-tv="selo-rec">
          {c.gravacao.estado === 'pausa' ? t('tv.topo.pausa') : c.gravacao.e4k ? t('tv.topo.rec4k') : t('tv.topo.rec')}
        </span>
      )}
      {noAr && (
        <span className="tv-selo tv-selo--live" data-tv="selo-ar">
          {destinosNoSelo ? t('tv.topo.aoVivoDestinos', { count: comChave }) : t('tv.topo.aoVivo')}
        </span>
      )}
    </>
  )
}

/** «CAM 2 · telefone da Ana», «2 · Ecrã» — o número é o do barramento. */
export function useNomeDaFonte(s: SessaoTv) {
  const { t } = useTranslation()
  return (id: string | null | undefined): string => {
    if (!id) return ''
    const f = s.registo.obter(id)
    if (!f) return ''
    const n = s.registo.numeroDe(id)
    const prefixo = f.tipo === 'camara' ? t('tv.fonte.cam', { n }) : n ? String(n) : ''
    return prefixo ? t('tv.fonte.rotulo', { prefixo, nome: f.nome }) : f.nome
  }
}

export function useTally(s: SessaoTv) {
  const { t } = useTranslation()
  return (id: string) => {
    const e = tallyDe(s.mesa, id)
    return { estado: e, texto: t(`tv.tally.${e}`) }
  }
}

/** Uma fonte no barramento: miniatura viva, tally, nome e tecla. */
export function CartaoDoBarramento({
  s,
  fonte,
  numero,
  onPrevia,
  onAr,
  mostrarTecla = true,
}: {
  s: SessaoTv
  fonte: FonteDaMesa
  numero: number
  onPrevia: () => void
  onAr: () => void
  mostrarTecla?: boolean
}) {
  const { t } = useTranslation()
  const tally = useTally(s)(fonte.id)
  return (
    <button
      type="button"
      className={cx('tv-mini', tally.estado === 'programa' && 'is-pgm', tally.estado === 'previa' && 'is-pre')}
      data-tv="fonte-barramento"
      data-tally={tally.estado}
      data-numero={numero}
      aria-label={t('tv.barramento.fonteAria', { n: numero, nome: fonte.nome, tally: tally.texto })}
      title={t('tv.barramento.dica')}
      onClick={(e) => (e.shiftKey ? onAr() : onPrevia())}
      onDoubleClick={onAr}
    >
      <span className="tv-mini__img">
        <Miniatura imagem={() => s.registo.imagem(fonte.id)} ajuste={s.registo.ajuste(fonte.id)} />
        <span className="tv-mini__num">{numero}</span>
        <span className="tv-mini__tally">{tally.texto}</span>
      </span>
      <span className="tv-mini__pe">
        <span className="tv-mini__nome">{fonte.nome}</span>
        {mostrarTecla && <span className="tv-mini__tecla">{numero}</span>}
      </span>
    </button>
  )
}

/** Um lugar do barramento sem fonte: leva às Fontes, onde se liga uma. */
export function LugarVazio({ numero, onAbrir }: { numero: number; onAbrir: () => void }) {
  const { t } = useTranslation()
  return (
    <button type="button" className="tv-mini tv-mini--vazio" onClick={onAbrir} aria-label={t('tv.barramento.vazioAria', { n: numero })}>
      <span className="tv-mini__img">
        <span className="tv-mini__num">{numero}</span>
        <span className="tv-mini__tally">{t('tv.tally.semFonte')}</span>
      </span>
      <span className="tv-mini__pe">
        <span className="tv-mini__nome tv-muted">{t('tv.barramento.ligar')}</span>
        <span className="tv-mini__tecla">{numero}</span>
      </span>
    </button>
  )
}
