/**
 * Partilhar uma gravação: link público (palavra-passe e expiração opcionais)
 * e acesso directo por pessoa. O servidor decide quem pode partilhar — só quem
 * carregou a gravação —, e um 403 mostra-se como a mensagem que ele devolve.
 */
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorMessage,
  createRecordingLink,
  getRecordingLink,
  isAbort,
  listRecordingShares,
  RecordingItem,
  revokeRecordingLink,
  searchUsers,
  ShareLink,
  shareRecording,
  unshareRecording,
  User,
} from '../../api'
import { Alert, Avatar, Button, Dialog, Field, IconButton, Skeleton, TextInput } from '../../ui/kit'
import { formatDateTime, recordingName } from './format'

type Link = { s: 'loading' } | { s: 'none' } | { s: 'active'; link: ShareLink } | { s: 'error'; msg: string }

export default function ShareDialog({ rec, onClose }: { rec: RecordingItem; onClose: () => void }) {
  const { t } = useTranslation()
  return (
    <Dialog title={t('recordings.partilha.titulo', { name: recordingName(rec) })} onClose={onClose} wide>
      <PublicLink rec={rec} />
      <hr className="rec-share__divider" />
      <People rec={rec} />
    </Dialog>
  )
}

function linkUrl(token: string) {
  return [location.origin, '/#/share/', token].join('')
}

function PublicLink({ rec }: { rec: RecordingItem }) {
  const { t, i18n } = useTranslation()
  const [link, setLink] = useState<Link>({ s: 'loading' })
  const [password, setPassword] = useState('')
  const [expiry, setExpiry] = useState('')
  const [busy, setBusy] = useState(false)
  const [confirmRevoke, setConfirmRevoke] = useState(false)
  const [copied, setCopied] = useState(false)
  const [err, setErr] = useState('')

  useEffect(() => {
    let live = true
    getRecordingLink(rec.id)
      .then((l) => live && setLink(l ? { s: 'active', link: l } : { s: 'none' }))
      .catch((e) => live && !isAbort(e) && setLink({ s: 'error', msg: apiErrorMessage(e, t('ui.erroCarregar')) }))
    return () => {
      live = false
    }
  }, [rec.id, t])

  async function generate() {
    setBusy(true)
    setErr('')
    try {
      const l = await createRecordingLink(rec.id, {
        password: password || undefined,
        expires_at: expiry ? new Date(expiry).toISOString() : null,
      })
      setLink({ s: 'active', link: l })
      setPassword('')
    } catch (e) {
      setErr(apiErrorMessage(e, t('recordings.partilha.erro')))
    } finally {
      setBusy(false)
    }
  }

  async function revoke(thenRenew: boolean) {
    setBusy(true)
    setErr('')
    try {
      await revokeRecordingLink(rec.id)
      setConfirmRevoke(false)
      if (thenRenew) {
        const l = await createRecordingLink(rec.id, { expires_at: null })
        setLink({ s: 'active', link: l })
      } else {
        setLink({ s: 'none' })
      }
    } catch (e) {
      setErr(apiErrorMessage(e, t('recordings.partilha.erro')))
    } finally {
      setBusy(false)
    }
  }

  function copy(token: string) {
    navigator.clipboard
      .writeText(linkUrl(token))
      .then(() => {
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      })
      .catch(() => setErr(t('recordings.partilha.erroCopiar')))
  }

  return (
    <section className="rec-share__section" aria-labelledby="rec-share-link">
      <h3 id="rec-share-link" className="rec-share__heading">
        {t('recordings.partilha.linkPublico')}
      </h3>
      {link.s === 'loading' && <Skeleton h={32} />}
      {link.s === 'error' && <Alert tone="danger">{link.msg}</Alert>}
      {link.s === 'none' && (
        <>
          <p className="rec-share__hint">{t('recordings.partilha.linkDica')}</p>
          <div className="rec-share__options">
            <Field label={t('recordings.partilha.password')} htmlFor="rec-link-pw">
              <TextInput
                id="rec-link-pw"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </Field>
            <Field label={t('recordings.partilha.expira')} htmlFor="rec-link-exp">
              <TextInput
                id="rec-link-exp"
                type="datetime-local"
                value={expiry}
                min={new Date().toISOString().slice(0, 16)}
                onChange={(e) => setExpiry(e.target.value)}
              />
            </Field>
          </div>
          <div>
            <Button variant="primary" size="sm" icon="link" busy={busy} onClick={() => void generate()}>
              {t('recordings.partilha.gerar')}
            </Button>
          </div>
        </>
      )}
      {link.s === 'active' && (
        <>
          <div className="rec-share__url">
            <TextInput code readOnly value={linkUrl(link.link.token)} aria-label={t('recordings.partilha.linkPublico')} onFocus={(e) => e.target.select()} />
            <IconButton
              icon={copied ? 'check' : 'copy'}
              label={copied ? t('recordings.partilha.copiado') : t('recordings.partilha.copiar')}
              onClick={() => copy(link.link.token)}
            />
          </div>
          <p className="rec-share__hint dx-num">
            {link.link.expires_at
              ? t('recordings.partilha.expiraEm', { date: formatDateTime(link.link.expires_at, i18n.language) })
              : t('recordings.partilha.semExpiracao')}
          </p>
          {/* Revogar pede confirmação num SEGUNDO botão noutra posição: um
              duplo-clique em «Revogar» não pode acertar em «Gerar link». */}
          {confirmRevoke ? (
            <div className="rec-share__confirm" role="group" aria-label={t('recordings.partilha.revogar')}>
              <span>{t('recordings.partilha.revogarAviso')}</span>
              <div className="rec-share__row">
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirmRevoke(false)}>
                  {t('ui.cancelar')}
                </Button>
                <Button size="sm" variant="danger" busy={busy} onClick={() => void revoke(false)}>
                  {t('recordings.partilha.confirmarRevogar')}
                </Button>
              </div>
            </div>
          ) : (
            <div className="rec-share__row">
              <Button size="sm" variant="outline" icon="refresh" disabled={busy} onClick={() => void revoke(true)}>
                {t('recordings.partilha.renovar')}
              </Button>
              <Button size="sm" variant="ghost" icon="ban" disabled={busy} onClick={() => setConfirmRevoke(true)}>
                {t('recordings.partilha.revogar')}
              </Button>
            </div>
          )}
        </>
      )}
      {err && <Alert tone="danger">{err}</Alert>}
    </section>
  )
}

function People({ rec }: { rec: RecordingItem }) {
  const { t } = useTranslation()
  const [shared, setShared] = useState<User[] | null>(null)
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<User[]>([])
  const [busy, setBusy] = useState<string | null>(null)
  const [err, setErr] = useState('')

  const load = useCallback(() => {
    listRecordingShares(rec.id)
      .then(setShared)
      .catch((e) => {
        if (isAbort(e)) return
        setShared([])
        setErr(apiErrorMessage(e, t('ui.erroCarregar')))
      })
  }, [rec.id, t])
  useEffect(load, [load])

  useEffect(() => {
    const q = query.trim()
    if (q.length < 2) {
      setResults([])
      return
    }
    let live = true
    const timer = setTimeout(() => {
      searchUsers(q)
        .then((r) => live && setResults(r))
        .catch(() => live && setResults([]))
    }, 250)
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [query])

  async function add(u: User) {
    setBusy(u.id)
    setErr('')
    try {
      await shareRecording(rec.id, u.id)
      setQuery('')
      setResults([])
      load()
    } catch (e) {
      setErr(apiErrorMessage(e, t('recordings.partilha.erro')))
    } finally {
      setBusy(null)
    }
  }

  async function remove(u: User) {
    setBusy(u.id)
    setErr('')
    try {
      await unshareRecording(rec.id, u.id)
      load()
    } catch (e) {
      setErr(apiErrorMessage(e, t('recordings.partilha.erro')))
    } finally {
      setBusy(null)
    }
  }

  const sharedIds = new Set((shared ?? []).map((u) => u.id))

  return (
    <section className="rec-share__section" aria-labelledby="rec-share-people">
      <h3 id="rec-share-people" className="rec-share__heading">
        {t('recordings.partilha.pessoas')}
      </h3>
      <p className="rec-share__hint">{t('recordings.partilha.pessoasDica')}</p>
      <TextInput
        type="search"
        autoComplete="off"
        placeholder={t('recordings.partilha.procurar')}
        aria-label={t('recordings.partilha.procurar')}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {results.length > 0 && (
        <ul className="rec-people" aria-label={t('recordings.partilha.resultados')}>
          {results.map((u) => (
            <li key={u.id} className="rec-people__item">
              <Avatar name={u.username} size={26} />
              <span className="rec-people__id">
                <strong>{u.username}</strong>
                <span>{u.email}</span>
              </span>
              {sharedIds.has(u.id) ? (
                <span className="rec-people__note">{t('recordings.partilha.jaTemAcesso')}</span>
              ) : (
                <IconButton
                  icon="userPlus"
                  label={t('recordings.partilha.adicionar', { name: u.username })}
                  disabled={busy === u.id}
                  onClick={() => void add(u)}
                />
              )}
            </li>
          ))}
        </ul>
      )}
      <h4 className="rec-share__sub">{t('recordings.partilha.comAcesso', { count: shared?.length ?? 0 })}</h4>
      {shared === null ? (
        <Skeleton h={26} />
      ) : shared.length === 0 ? (
        <p className="rec-share__hint">{t('recordings.partilha.ninguem')}</p>
      ) : (
        <ul className="rec-people">
          {shared.map((u) => (
            <li key={u.id} className="rec-people__item">
              <Avatar name={u.username} size={26} />
              <span className="rec-people__id">
                <strong>{u.username}</strong>
                <span>{u.email}</span>
              </span>
              <IconButton
                icon="trash"
                bare
                label={t('recordings.partilha.remover', { name: u.username })}
                disabled={busy === u.id}
                onClick={() => void remove(u)}
              />
            </li>
          ))}
        </ul>
      )}
      {err && <Alert tone="danger">{err}</Alert>}
    </section>
  )
}
