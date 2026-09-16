/**
 * Banco de cenas: as composições guardadas no dispositivo (IndexedDB, ver
 * `cenas.ts`). Um clique aplica; «Nova cena» guarda o palco tal como está.
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button, cx, IconButton, TextInput } from '../ui/kit'
import type { CenaGuardada } from './cenas'

/** A miniatura é um Blob; o URL vive enquanto o cartão existe. */
function Miniatura({ blob }: { blob: Blob | null }) {
  const [url, setUrl] = useState('')
  useEffect(() => {
    if (!blob) return
    const u = URL.createObjectURL(blob)
    setUrl(u)
    return () => URL.revokeObjectURL(u)
  }, [blob])
  return <span className="st-scene__thumb" style={url ? { backgroundImage: `url(${url})` } : undefined} aria-hidden="true" />
}

export default function CenasPanel({
  cenas,
  activa,
  indisponivel,
  onAplicar,
  onNova,
  onApagar,
}: {
  cenas: CenaGuardada[]
  activa: string
  indisponivel: boolean
  onAplicar: (c: CenaGuardada) => void
  onNova: (nome: string) => Promise<void>
  onApagar: (id: string) => void
}) {
  const { t } = useTranslation()
  const [aCriar, setACriar] = useState(false)
  const [nome, setNome] = useState('')
  const [aGuardar, setAGuardar] = useState(false)

  async function submeter(e: FormEvent) {
    e.preventDefault()
    const n = nome.trim()
    if (!n) return
    setAGuardar(true)
    try {
      await onNova(n)
      setNome('')
      setACriar(false)
    } finally {
      setAGuardar(false)
    }
  }

  return (
    <section className="st-group" data-studio-grupo="cenas" aria-labelledby="st-cenas-h">
      <h2 id="st-cenas-h" className="st-group__title">
        {t('studio.cenas.titulo')}
      </h2>
      {indisponivel && <p className="st-note st-note--warn">{t('studio.cenas.indisponivel')}</p>}
      <ul className="st-scenes">
        {cenas.map((c) => (
          <li key={c.id} className={cx('st-scene', activa === c.id && 'is-active')}>
            <button
              type="button"
              className="st-scene__apply"
              aria-pressed={activa === c.id}
              data-studio="cena"
              onClick={() => onAplicar(c)}
            >
              <Miniatura blob={c.miniatura} />
              <span className="st-scene__name">{c.nome}</span>
            </button>
            <IconButton icon="trash" bare label={t('studio.cenas.apagar', { nome: c.nome })} onClick={() => onApagar(c.id)} />
          </li>
        ))}
      </ul>
      {aCriar ? (
        <form className="st-scene-new" onSubmit={(e) => void submeter(e)}>
          <TextInput
            autoFocus
            value={nome}
            maxLength={40}
            placeholder={t('studio.cenas.nomePh')}
            aria-label={t('studio.cenas.nome')}
            data-studio="cena-nome"
            onChange={(e) => setNome(e.target.value)}
          />
          <div className="st-actions">
            <Button type="submit" size="sm" variant="primary" busy={aGuardar} disabled={!nome.trim()} data-studio="cena-guardar">
              {t('studio.cenas.guardar')}
            </Button>
            <Button type="button" size="sm" variant="ghost" onClick={() => setACriar(false)}>
              {t('studio.cenas.cancelar')}
            </Button>
          </div>
        </form>
      ) : (
        <button type="button" className="st-scene st-scene--new" data-studio="cena-nova" onClick={() => setACriar(true)} disabled={indisponivel}>
          <span className="st-scene__thumb st-scene__thumb--plus" aria-hidden="true">
            +
          </span>
          <span className="st-scene__name">{t('studio.cenas.nova')}</span>
        </button>
      )}
      <p className="st-note">{t('studio.cenas.nota')}</p>
    </section>
  )
}
