/**
 * «Telemóvel como câmara» — o emparelhamento por código e QR (ADR-0014 §2.1).
 *
 * O servidor serve isto desde que o `studio.rs` existe: emite um código
 * `XXXX-XXXX` de dez minutos, e o telefone resgata-o em `POST
 * /api/studio-pairings` recebendo a SALA e um token de fonte. O cartão daqui
 * dizia «a aguardar servidor» porque o web nunca tinha sido ligado — não
 * porque faltasse servidor.
 *
 * O código mostra-se UMA vez: a base só guarda o localizador e o SHA-256 do
 * segredo. Quem fechar este painel sem apontar o código emite outro.
 *
 * O QR leva o URL do telefone, não o código cru: quem lê com a câmara cai
 * directamente na página certa. Para quem prefere escrever, o código fica
 * visível ao lado.
 */
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorCode,
  createPairingCode,
  listPairingCodes,
  listStudioSources,
  type NewPairingCode,
  type PairingCode,
  revokePairingCode,
  type StudioSource,
} from '../../../api'
import { estudioActual } from '../../../studio/estudio'
import { cx } from '../../../ui/kit'
import { Icon } from '../../../ui/icons'
import { BotaoTv, Cabeca, Cartao } from './pecas'

/** O URL que o telefone abre. O código vai no fragmento, como as outras rotas. */
export function urlDoTelefone(code: string): string {
  return `${globalThis.location?.origin ?? ''}/#/emparelhar/${encodeURIComponent(code)}`
}

function Conta({ ate }: { ate: string }) {
  const { t } = useTranslation()
  const [agora, setAgora] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setAgora(Date.now()), 1000)
    return () => clearInterval(id)
  }, [])
  const segundos = Math.max(0, Math.round((new Date(ate).getTime() - agora) / 1000))
  const m = Math.floor(segundos / 60)
  const s = segundos % 60
  return (
    <span className="tv-mono-85" data-tv="conta-decrescente">
      {segundos > 0 ? `${m}:${String(s).padStart(2, '0')}` : t('tv.emparelhar.expirado')}
    </span>
  )
}

export function Emparelhar() {
  const { t } = useTranslation()
  const [orgId, setOrgId] = useState('')
  const [studioId, setStudioId] = useState('')
  const [codigos, setCodigos] = useState<PairingCode[]>([])
  const [fontes, setFontes] = useState<StudioSource[]>([])
  const [novo, setNovo] = useState<NewPairingCode | null>(null)
  const [qr, setQr] = useState('')
  const [erro, setErro] = useState('')
  const [ocupado, setOcupado] = useState(false)

  // Um estúdio por organização chega para já: o ecrã não tem onde escolher
  // entre vários, e criar um à socapa é melhor do que um painel vazio que não
  // diz porquê. Quando houver mais do que um, isto passa a selector.
  const carregar = useCallback(async () => {
    try {
      const actual = await estudioActual(t('tv.emparelhar.nomeEstudio'))
      if (!actual) return setErro(t('tv.emparelhar.semOrg'))
      setOrgId(actual.orgId)
      setStudioId(actual.estudio.id)
      const [cods, fs] = await Promise.all([
        listPairingCodes(actual.orgId, actual.estudio.id),
        listStudioSources(actual.orgId, actual.estudio.id),
      ])
      setCodigos(cods.items)
      setFontes(fs.items.filter((f) => !f.revoked_at))
    } catch (e) {
      setErro(apiErrorCode(e) ?? t('tv.emparelhar.falhou'))
    }
  }, [t])

  useEffect(() => {
    void carregar()
    // O telefone liga-se DEPOIS de alguém ler o QR, e quem sabe quando é o
    // servidor: sem relermos, o cartão ficava a dizer que não há ninguém com
    // uma câmara já ligada do outro lado.
    const id = setInterval(() => void carregar(), 5000)
    return () => clearInterval(id)
  }, [carregar])

  const emitir = async () => {
    if (!orgId || !studioId || ocupado) return
    setOcupado(true)
    setErro('')
    try {
      const c = await createPairingCode(orgId, studioId)
      setNovo(c)
      // O gerador é o mesmo do MFA e dos ramais; carrega-se a pedido.
      const { toString } = await import('qrcode')
      setQr(await toString(urlDoTelefone(c.code), { type: 'svg', margin: 1 }))
      setCodigos((await listPairingCodes(orgId, studioId)).items)
    } catch (e) {
      setErro(apiErrorCode(e) ?? t('tv.emparelhar.falhou'))
    } finally {
      setOcupado(false)
    }
  }

  const revogar = async (id: string) => {
    if (!orgId || !studioId) return
    try {
      await revokePairingCode(orgId, studioId, id)
      if (novo?.id === id) {
        setNovo(null)
        setQr('')
      }
      setCodigos((await listPairingCodes(orgId, studioId)).items)
    } catch (e) {
      setErro(apiErrorCode(e) ?? t('tv.emparelhar.falhou'))
    }
  }

  const activos = codigos.filter((c) => c.state === 'active')

  return (
    <Cartao como="section" style={{ gap: 7 }} data-tv="emparelhar">
      <Cabeca>
        <h3 className="tv-t2">
          <Icon name="phone" /> {t('tv.fontes.app')}
        </h3>
        <span className="tv-dir tv-mono-85">{t('tv.emparelhar.nActivos', { count: activos.length })}</span>
      </Cabeca>
      <p className="tv-nota">{t('tv.emparelhar.nota')}</p>

      {erro && (
        <p className="tv-nota tv-erro" role="alert">
          {erro}
        </p>
      )}

      <BotaoTv variante="forte" disabled={!studioId || ocupado} onClick={() => void emitir()}>
        {t('tv.emparelhar.emitir')}
      </BotaoTv>

      {novo && (
        <div className="tv-emparelhar" data-tv="codigo-novo">
          {qr && (
            <div
              className="tv-emparelhar__qr"
              aria-label={t('tv.emparelhar.qrAria')}
              /* O SVG vem do gerador local (`qrcode`), não da rede. */
              dangerouslySetInnerHTML={{ __html: qr }}
            />
          )}
          <div className="tv-emparelhar__lado">
            <span className="tv-emparelhar__codigo dx-num">{novo.code}</span>
            <span className="tv-nota">{t('tv.emparelhar.umaVez')}</span>
            <div className="tv-kv">
              <span>{t('tv.emparelhar.expiraEm')}</span>
              <Conta ate={novo.expires_at} />
            </div>
            <div className="tv-kv">
              <span>{t('tv.emparelhar.sala')}</span>
              <span className="dx-num">{novo.room_code}</span>
            </div>
            <span className="tv-nota">{urlDoTelefone(novo.code)}</span>
          </div>
        </div>
      )}

      {fontes.length > 0 && (
        <ul className="tv-lista" data-tv="fontes-emparelhadas">
          {fontes.map((f) => (
            <li key={f.id} className="tv-kv">
              <span>
                <span className="tv-tag">CAM {f.number}</span> {f.label}
              </span>
              <span className={cx('tv-dir tv-mono-85', f.connected ? 'tv-ok' : 'tv-muted')}>
                {f.connected ? t('tv.emparelhar.ligada') : t('tv.emparelhar.desligada')}
              </span>
            </li>
          ))}
        </ul>
      )}

      {activos.length > 0 && (
        <ul className="tv-lista" data-tv="codigos-activos">
          {activos.map((c) => (
            <li key={c.id} className="tv-kv">
              <span>
                {c.label || t('tv.emparelhar.semEtiqueta')}
                {c.number != null && <span className="tv-tag">CAM {c.number}</span>}
              </span>
              <span className="tv-dir" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <Conta ate={c.expires_at} />
                <span className="tv-mono-85">
                  {t('tv.emparelhar.tentativas', { usadas: c.attempts, max: c.max_attempts })}
                </span>
                <BotaoTv mini onClick={() => void revogar(c.id)}>
                  {t('tv.emparelhar.revogar')}
                </BotaoTv>
              </span>
            </li>
          ))}
        </ul>
      )}
    </Cartao>
  )
}
