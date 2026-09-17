/**
 * Formatação das gravações e dos quadros: tudo pelo `Intl` do idioma activo,
 * nenhuma unidade escrita à mão. Os números vêm do servidor (tamanho, data)
 * ou do próprio ficheiro de vídeo (duração, resolução) — nunca do template.
 */
import type { RecordingItem } from '../../api'
import { avatarTone } from '../../ui/kit'

export function formatBytes(bytes: number, lang: string): string {
  const gb = bytes / 1024 ** 3
  if (gb >= 1) {
    return new Intl.NumberFormat(lang, { style: 'unit', unit: 'gigabyte', maximumFractionDigits: 1 }).format(gb)
  }
  const mb = bytes / 1024 ** 2
  return new Intl.NumberFormat(lang, { style: 'unit', unit: 'megabyte', maximumFractionDigits: 1 }).format(mb)
}

export function formatDateTime(iso: string, lang: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString(lang, { dateStyle: 'medium', timeStyle: 'short' })
}

/** Data e hora curtas, para colunas de tabela. */
export function formatDateTimeShort(iso: string, lang: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString(lang, { dateStyle: 'short', timeStyle: 'short' })
}

export function formatDate(iso: string, lang: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleDateString(lang, { day: 'numeric', month: 'short', year: 'numeric' })
}

/** h:mm:ss ou m:ss — o formato do leitor, em algarismos só. */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const r = s % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? [h, pad(m), pad(r)].join(':') : [m, pad(r)].join(':')
}

/** Nome visível: o ficheiro sem a extensão do contentor. */
export function recordingName(r: Pick<RecordingItem, 'filename'>): string {
  return r.filename.replace(/\.(webm|mp4|mkv)$/i, '')
}

export const isFailed = (r: Pick<RecordingItem, 'status'>) => r.status === 'failed'

/** Miniatura duotone estável por nome, com o tom da paleta do kit. */
export function thumbBackground(seed: string): string {
  return ['linear-gradient(140deg, ', avatarTone(seed), ', var(--stage))'].join('')
}

/** «04 set.» — dia e mês, para listas compactas. */
export function formatDayMonth(iso: string, lang: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleDateString(lang, { day: '2-digit', month: 'short' })
}
