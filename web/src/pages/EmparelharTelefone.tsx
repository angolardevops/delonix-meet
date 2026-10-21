/**
 * O lado do TELEFONE do emparelhamento (ADR-0014 §2.1).
 *
 * Quem lê o QR do Estúdio cai aqui com o código no URL e não escreve nada: a
 * página resgata-o e segue para a câmara. Quem prefere escrever o código
 * abre `#/emparelhar` sem ele e tem a caixa.
 *
 * Esta página é ANÓNIMA de propósito — o telefone não tem conta, e a
 * credencial é o código, que vive dez minutos e se gasta à primeira. O
 * servidor responde o mesmo a código inexistente, expirado, já usado,
 * queimado ou com segredo errado: a recusa não ensina nada a quem tenta.
 *
 * O que fazemos com o resgate: guardamos o token de fonte e entramos na sala
 * pela página de câmara que já existe (`#/telemovel/<sala>`). O token vale 12
 * horas, uma sessão de estúdio inteira.
 */
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorCode, ApiError, redeemPairingCode } from '../api'
import { guardarFonte } from '../fonteDoEstudio'
import type { Pairing } from '../api'
import { Alert, Button, Spinner } from '../ui/kit'
import '../ui/phoneCamera.css'

/** `XXXX-XXXX` em Crockford base32 — o que o servidor aceita. */
const FORMATO = /^[0-9A-HJKMNP-TV-Z]{4}-?[0-9A-HJKMNP-TV-Z]{4}$/i

export function normalizar(bruto: string): string {
  const limpo = bruto.trim().toUpperCase().replace(/\s+/g, '')
  return limpo.includes('-') || limpo.length !== 8 ? limpo : `${limpo.slice(0, 4)}-${limpo.slice(4)}`
}

export default function EmparelharTelefone({ code }: { code?: string }) {
  const { t } = useTranslation()
  const [valor, setValor] = useState(code ? normalizar(code) : '')
  const [aResgatar, setAResgatar] = useState(false)
  /** Resgatado, à espera do consentimento para ir ao ar. */
  const [pronto, setPronto] = useState<Pairing | null>(null)
  const [erro, setErro] = useState('')

  const resgatar = useCallback(async (bruto: string) => {
    const codigo = normalizar(bruto)
    if (!FORMATO.test(codigo)) return setErro(t('telemovel.emparelhar.formato'))
    setAResgatar(true)
    setErro('')
    try {
      const p = await redeemPairingCode(codigo, {
        model: navigator.userAgent.slice(0, 64),
        platform: navigator.platform || 'web',
      })
      // Guardado ANTES de mudar o endereço: a página da câmara lê-o ao
      // arrancar, e é isto que a faz entrar como FONTE e não como convidado.
      guardarFonte({
        code: p.room_code,
        roomToken: p.source_token,
        iceServers: p.ice_servers,
        sourceId: p.source_id,
        studioId: p.studio_id,
        number: p.number,
        label: p.label,
        expiraEm: new Date(p.expires_at).getTime(),
      })
      // NÃO se entra aqui. Quem abriu este URL pode ser uma PESSOA a quem
      // mandaram o link, não um aparelho de estúdio: publicar câmara e
      // microfone sem um clique era pôr alguém no ar de uma organização que
      // nem lhe foi nomeada. O passo seguinte é dela.
      setPronto(p)
    } catch (e) {
      const c = apiErrorCode(e)
      /**
       * O CÓDIGO nunca vai ao ecrã. Antes, o último ramo era `c ?? falhou`:
       * com um código presente mostrava-se `studio.invalid_device` num quadro
       * vermelho, em inglês qualquer que fosse a língua, e sem dizer o que
       * fazer — a quem está ao telefone, que não sabe o que é um código de
       * erro nosso. Vai para a consola, que é onde serve para diagnóstico.
       */
      if (c) console.warn('emparelhamento recusado:', c)
      const demasiadosPedidos = e instanceof ApiError && e.status === 429
      setErro(
        demasiadosPedidos
          ? t('telemovel.emparelhar.demasiados')
          : c === 'studio.pairing_invalid'
            ? t('telemovel.emparelhar.invalido')
            : c === 'studio.pairing_malformed'
              ? t('telemovel.emparelhar.formato')
              : c === 'studio.full'
                ? t('telemovel.emparelhar.cheio')
                : t('telemovel.emparelhar.falhou'),
      )
    } finally {
      setAResgatar(false)
    }
  }, [t])

  return (
    <div className="emp-tel">
      <h1>{t('telemovel.emparelhar.titulo')}</h1>
      <p className="dx-muted">{t('telemovel.emparelhar.nota')}</p>

      {erro && <Alert tone="danger">{erro}</Alert>}

      {pronto ? (
        /*
         * O passo que faltava. Diz o que vai acontecer ANTES de acontecer:
         * que câmara se é no estúdio, e que a câmara e o microfone deste
         * telefone vão ao ar. O nome do estúdio não está aqui porque o resgate
         * não o devolve — está nomeado como a lacuna que resta.
         */
        <>
          <Alert tone="warning">{t('telemovel.emparelhar.vaiAoAr')}</Alert>
          <p className="dx-num">{pronto.label}</p>
          <Button variant="primary" onClick={() => (location.hash = `/telemovel/${pronto.room_code}`)}>
            {t('telemovel.emparelhar.comecar')}
          </Button>
        </>
      ) : aResgatar ? (
        <Spinner label={t('telemovel.emparelhar.aEntrar')} />
      ) : (
        <>
          <input
            className="dx-input dx-num"
            inputMode="text"
            autoCapitalize="characters"
            autoComplete="off"
            spellCheck={false}
            placeholder="XXXX-XXXX"
            aria-label={t('telemovel.emparelhar.campo')}
            value={valor}
            onChange={(e) => setValor(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && void resgatar(valor)}
          />
          <Button variant="primary" disabled={!valor} onClick={() => void resgatar(valor)}>
            {t('telemovel.emparelhar.entrar')}
          </Button>
        </>
      )}
    </div>
  )
}
