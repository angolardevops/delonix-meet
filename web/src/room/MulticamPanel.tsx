import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import type { Cena } from './compositor'
import { Icon } from '../ui/icons'
import { Alert, Button, Field, IconButton, Segmented, StatusBadge, TextInput } from '../ui/kit'
import { Since } from './Clocks'
import { MAX_MULTICAM_DESTINOS, type Multicam } from './useMulticam'
import type { RemotePeer } from './useRoomCore'

/** Multicâmara: compõe a sala num quadro e emite-o para plataformas RTMP. */
export function MulticamPanel({ multicam, peers }: { multicam: Multicam; peers: RemotePeer[] }) {
  const { t } = useTranslation()
  const [verChave, setVerChave] = useState<Record<number, boolean>>({})
  const m = multicam
  const noAr = m.estado.fase === 'no-ar'
  const pessoas = [{ id: 'eu', nome: currentUser()?.username ?? '' }, ...peers.map((p) => ({ id: p.peerId, nome: p.username }))]

  return (
    <div className="rm-scroll">
      <div className="rm-multicam__preview" ref={m.previewRef} aria-label={t('room.multicam.previsualizacao')} />
      {m.estado.fase === 'no-ar' && (
        <p className="rm-block__row dx-num">
          <StatusBadge tone="live">{t('room.topo.aoVivo')}</StatusBadge>
          <Since desde={m.estado.desde} render={(txt) => <span>{txt}</span>} />
          <span className="dx-muted">{t('room.multicam.megabytes', { n: (m.estado.bytes / 1_048_576).toFixed(1) })}</span>
        </p>
      )}

      <section className="rm-block" aria-labelledby="rm-mc-cena">
        <h3 id="rm-mc-cena" className="rm-block__title">
          <Icon name="layers" size={13} />
          {t('room.multicam.cena')}
        </h3>
        <Segmented<Cena>
          label={t('room.multicam.cena')}
          value={m.cena}
          onChange={m.setCena}
          options={[
            { value: 'grelha', label: t('room.multicam.cenaGrelha') },
            { value: 'solo', label: t('room.multicam.cenaSolo') },
            { value: 'lado-a-lado', label: t('room.multicam.cenaLadoALado') },
          ]}
        />
        {m.cena !== 'grelha' && (
          <>
            <span className="dx-muted">{m.cena === 'solo' ? t('room.multicam.escolherUm') : t('room.multicam.escolherDois')}</span>
            <div className="dx-chips">
              {pessoas.map((p) => (
                <button key={p.id} type="button" className="dx-chip" aria-pressed={m.focoIds.includes(p.id)} onClick={() => m.toggleFoco(p.id)}>
                  {p.nome}
                </button>
              ))}
            </div>
          </>
        )}
      </section>

      <section className="rm-block" aria-labelledby="rm-mc-dest">
        <h3 id="rm-mc-dest" className="rm-block__title">
          <Icon name="live" size={13} />
          {t('room.multicam.destinos')}
          <span className="dx-spacer" />
          <span className="dx-num dx-muted">
            {m.destinos.length}/{MAX_MULTICAM_DESTINOS}
          </span>
        </h3>
        {m.destinos.map((d, i) => (
          <fieldset key={i} className="rm-dest" disabled={noAr || m.estado.fase === 'a-ligar'}>
            <legend className="dx-field__label">
              {d.rotulo || t('room.multicam.destinoN', { n: i + 1 })}
              {m.destinos.length > 1 && (
                <IconButton icon="x" bare label={t('room.multicam.removerDestino', { n: i + 1 })} onClick={() => m.removeDestino(i)} />
              )}
            </legend>
            <Field label={t('room.multicam.servidor')} htmlFor={`rm-mc-url-${i}`}>
              <TextInput
                id={`rm-mc-url-${i}`}
                code
                value={d.url}
                autoComplete="off"
                placeholder="rtmp://…"
                onChange={(e) => m.updateDestino(i, { url: e.target.value })}
              />
            </Field>
            <Field label={t('room.multicam.chave')} htmlFor={`rm-mc-key-${i}`}>
              <div className="rm-pass">
                <TextInput
                  id={`rm-mc-key-${i}`}
                  type={verChave[i] ? 'text' : 'password'}
                  value={d.chave}
                  autoComplete="off"
                  onChange={(e) => m.updateDestino(i, { chave: e.target.value })}
                />
                <button
                  type="button"
                  className="dx-iconbtn"
                  aria-pressed={!!verChave[i]}
                  aria-label={verChave[i] ? t('room.e2ee.esconder') : t('room.e2ee.mostrar')}
                  onClick={() => setVerChave((v) => ({ ...v, [i]: !v[i] }))}
                >
                  <Icon name="eye" />
                </button>
              </div>
            </Field>
            <Field label={t('room.multicam.rotulo')} htmlFor={`rm-mc-lbl-${i}`}>
              <TextInput id={`rm-mc-lbl-${i}`} value={d.rotulo ?? ''} autoComplete="off" onChange={(e) => m.updateDestino(i, { rotulo: e.target.value })} />
            </Field>
          </fieldset>
        ))}
        {m.destinos.length < MAX_MULTICAM_DESTINOS ? (
          <Button size="sm" variant="ghost" icon="plus" disabled={noAr} onClick={m.addDestino}>
            {t('room.multicam.adicionarDestino')}
          </Button>
        ) : (
          <p className="dx-muted">{t('room.multicam.limite', { n: MAX_MULTICAM_DESTINOS })}</p>
        )}
      </section>

      {m.estado.fase === 'erro' && <Alert tone="danger">{m.estado.motivo}</Alert>}
      {noAr ? (
        <Button variant="danger" block icon="stop" onClick={() => void m.stopLive()}>
          {t('room.multicam.terminar')}
        </Button>
      ) : (
        <Button
          variant="live"
          block
          icon="live"
          busy={m.estado.fase === 'a-ligar'}
          disabled={!m.destinos.some((d) => d.chave.trim())}
          onClick={() => void m.goLive()}
        >
          {m.estado.fase === 'a-ligar' ? t('room.multicam.aLigar') : t('room.multicam.irParaOAr')}
        </Button>
      )}
    </div>
  )
}
