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
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorCode, redeemPairingCode } from '../api'
import { guardarFonte } from '../fonteDoEstudio'
import { Alert, Button, Spinner } from '../ui/kit'

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
  const [erro, setErro] = useState('')
  // O código do URL resgata-se UMA vez: sem isto, um erro de rede que mude o
  // estado voltava a disparar o efeito e queimava tentativas do código.
  const jaTentou = useRef(false)

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
      location.hash = `/telemovel/${p.room_code}`
    } catch (e) {
      const c = apiErrorCode(e)
      setErro(
        c === 'studio.pairing_invalid'
          ? t('telemovel.emparelhar.invalido')
          : c === 'studio.full'
            ? t('telemovel.emparelhar.cheio')
            : (c ?? t('telemovel.emparelhar.falhou')),
      )
    } finally {
      setAResgatar(false)
    }
  }, [t])

  useEffect(() => {
    // O `jaTentou` é que garante a tentativa ÚNICA, não a lista de
    // dependências: um código do URL resgatado duas vezes queima tentativas.
    if (!code || jaTentou.current) return
    jaTentou.current = true
    void resgatar(code)
  }, [code, resgatar])

  return (
    <div className="dx-centro" style={{ display: 'grid', gap: 12, padding: 16, maxWidth: 360, margin: '0 auto' }}>
      <h1 className="dx-h2">{t('telemovel.emparelhar.titulo')}</h1>
      <p className="dx-muted">{t('telemovel.emparelhar.nota')}</p>

      {erro && <Alert tone="danger">{erro}</Alert>}

      {aResgatar ? (
        <Spinner />
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
