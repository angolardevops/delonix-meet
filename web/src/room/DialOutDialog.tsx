import { useTranslation } from 'react-i18next'
import { Icon } from '../ui/icons'
import { Alert, Button, Dialog, Field, IconButton, StatusBadge, TextInput } from '../ui/kit'
import { chaveDaFalha, estaVivo, tomDoEstado } from './dialOut'
import type { DialOutCtl } from './useDialOut'

/** «Ligar a…»: faz tocar um ramal da organização; quem atende entra na sala. */
export function DialOutDialog({ ctl }: { ctl: DialOutCtl }) {
  const { t } = useTranslation()
  return (
    <Dialog
      title={t('room.ligar.titulo')}
      onClose={ctl.close}
      footer={
        <Button variant="ghost" onClick={ctl.close}>
          {t('room.ligar.fechar')}
        </Button>
      }
    >
      <Alert tone="warning" icon="mic">
        {t('room.ligar.aviso')}
      </Alert>

      {ctl.carga === 'a-carregar' && <p className="dx-muted">{t('room.ligar.aCarregar')}</p>}
      {ctl.carga === 'sem-acesso' && <Alert tone="warning">{t('room.ligar.semAcesso')}</Alert>}
      {ctl.carga === 'erro' && <Alert tone="danger">{t('room.ligar.erro.ramais')}</Alert>}

      {ctl.carga === 'pronta' && (
        <>
          <Field label={t('room.ligar.pesquisar')} htmlFor="rm-dial-q" hint={t('room.ligar.pesquisarDica')}>
            <TextInput id="rm-dial-q" value={ctl.query} autoComplete="off" onChange={(e) => ctl.setQuery(e.target.value)} />
          </Field>
          {ctl.chamaveis.length === 0 ? (
            <p className="dx-muted">{t('room.ligar.semRamais')}</p>
          ) : (
            <ul className="rm-invite__list" aria-label={t('room.ligar.ramais')}>
              {ctl.chamaveis.map((c) => {
                const ocupado = ctl.ocupados.has(c.ext.id)
                return (
                  <li key={c.ext.id} className="rm-invite__item rm-invite__item--static">
                    <Icon name="phone" size={14} />
                    <span className="rm-invite__who">
                      <strong>{ctl.nomeDe(c)}</strong>
                      <small className="dx-muted">
                        {t('room.ligar.ramal', { numero: c.ext.extension })}
                        {ctl.varias ? ` · ${c.org}` : ''}
                      </small>
                    </span>
                    <Button
                      size="sm"
                      variant="primary"
                      icon="phone"
                      busy={ctl.busyId === c.ext.id}
                      disabled={ocupado || ctl.busyId !== null}
                      onClick={() => void ctl.ligar(c)}
                    >
                      {ocupado ? t('room.ligar.aChamar') : t('room.ligar.ligar')}
                    </Button>
                  </li>
                )
              })}
            </ul>
          )}
        </>
      )}

      {ctl.items.length > 0 && (
        <section aria-label={t('room.ligar.chamadas')}>
          <h3 className="dx-h4">{t('room.ligar.chamadas')}</h3>
          <ul className="rm-invite__list" role="status" aria-live="polite">
            {ctl.items.map((d) => {
              const falha = chaveDaFalha(d)
              return (
                <li key={d.id} className="rm-invite__item rm-invite__item--static">
                  <span className="rm-invite__who">
                    <strong>{d.display_name ?? d.extension ?? '—'}</strong>
                    <small className="dx-muted">{falha ? t(falha) : ''}</small>
                  </span>
                  <StatusBadge tone={tomDoEstado(d.status)}>{t(`room.ligar.estado.${d.status}`)}</StatusBadge>
                  {estaVivo(d.status) && (
                    <IconButton
                      icon="phoneOff"
                      bare
                      label={d.status === 'in_call' ? t('room.ligar.desligar') : t('room.ligar.cancelar')}
                      disabled={ctl.busyId === d.id}
                      onClick={() => void ctl.desligar(d)}
                    />
                  )}
                </li>
              )
            })}
          </ul>
        </section>
      )}
      {ctl.status && <Alert tone={ctl.status.tone}>{ctl.status.text}</Alert>}
    </Dialog>
  )
}
