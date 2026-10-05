/**
 * Entrar numa reunião SEM CONTA — o ecrã de quem recebeu um link.
 *
 * Pede um nome e mais nada. O servidor emite um bilhete de uma só sala
 * (`guestJoin`), e a entrada passa SEMPRE pela sala de espera: é o anfitrião
 * que admite. Quem tem conta segue para o login pelo botão de baixo, e fica
 * com o mesmo destino.
 *
 * As recusas dizem o que fazer a seguir, cada uma a sua: a sala que só aceita
 * contas manda para o login, o travão diz quanto falta, e o código que não
 * existe não finge que é um problema de rede.
 */
import { FormEvent, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { GUEST_NAME_MAX, GuestJoinError, type GuestJoinFailure } from '../../api'
import { entrarComoConvidado, nomeDeConvidado, type SessaoDeConvidado } from '../../convidado'
import { Icon } from '../../ui/icons'
import { Alert, Button, Field, TextInput } from '../../ui/kit'

export default function EntradaDeConvidado({
  code,
  onEntrou,
  onTenhoConta,
}: {
  code: string
  onEntrou: (s: SessaoDeConvidado) => void
  onTenhoConta: () => void
}) {
  const { t } = useTranslation()
  const [nome, setNome] = useState(nomeDeConvidado)
  const [aPedir, setAPedir] = useState(false)
  const [falha, setFalha] = useState<{ razao: GuestJoinFailure; espera: number | null } | null>(null)
  const campo = useRef<HTMLInputElement>(null)

  useEffect(() => {
    campo.current?.focus()
  }, [])

  async function submeter(e: FormEvent) {
    e.preventDefault()
    const limpo = nome.trim()
    if (!limpo || aPedir) return
    setFalha(null)
    setAPedir(true)
    try {
      onEntrou(await entrarComoConvidado(code, limpo))
    } catch (err) {
      const g = err instanceof GuestJoinError ? err : null
      setFalha({ razao: g?.reason ?? 'unavailable', espera: g?.retryAfterSecs ?? null })
    } finally {
      setAPedir(false)
    }
  }

  const mensagem: Record<GuestJoinFailure, string> = {
    closed: t('auth.convidado.erro.soContas'),
    not_found: t('auth.convidado.erro.naoExiste'),
    invalid_name: t('auth.convidado.erro.nome', { max: GUEST_NAME_MAX }),
    rate_limited: falha?.espera ? t('auth.convidado.erro.esperaSegundos', { count: falha.espera }) : t('auth.convidado.erro.espera'),
    unavailable: t('auth.convidado.erro.indisponivel'),
  }

  return (
    <div className="auth-form" data-testid="convidado-entrada">
      <div className="auth-pendente" role="status">
        <Icon name="door" />
        <div className="auth-pendente__texto">
          <span>
            {t('auth.convidado.reuniao')} <code className="dx-num">{code}</code>
          </span>
        </div>
      </div>

      <header className="auth-form__head">
        <h1>{t('auth.convidado.titulo')}</h1>
        <p className="dx-muted">{t('auth.convidado.explicacao')}</p>
      </header>

      <form className="auth-form__campos" onSubmit={submeter} noValidate>
        <Field label={t('auth.convidado.nome')} htmlFor="convidado-nome" hint={t('auth.convidado.nomeDica')}>
          <TextInput
            id="convidado-nome"
            ref={campo}
            name="display_name"
            large
            autoComplete="name"
            maxLength={GUEST_NAME_MAX}
            required
            value={nome}
            onChange={(e) => setNome(e.target.value)}
          />
        </Field>

        {falha && (
          <div className="auth-error" data-testid="convidado-erro">
            <Alert tone="danger">{mensagem[falha.razao]}</Alert>
          </div>
        )}

        <Button
          type="submit"
          variant="secondary"
          size="lg"
          block
          busy={aPedir}
          disabled={!nome.trim()}
          className="auth-submit"
          data-testid="convidado-entrar"
        >
          {t('auth.convidado.pedirEntrada')}
        </Button>
      </form>

      <p className="auth-form__troca">
        {t('auth.convidado.temConta')}{' '}
        <button type="button" className="auth-link" onClick={onTenhoConta} data-testid="convidado-tenho-conta">
          {t('auth.convidado.iniciarSessao')}
        </button>
      </p>
    </div>
  )
}
