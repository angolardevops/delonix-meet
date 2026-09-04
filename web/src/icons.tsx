/** Ícones SVG (traço 24px, estilo Material) — sem dependências externas. */

const base = {
  width: 22,
  height: 22,
  viewBox: '0 0 24 24',
  fill: 'currentColor',
} as const

export const MicIcon = () => (
  <svg {...base}>
    <path d="M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3Zm-1 7v-3.07A7 7 0 0 1 5 11h2a5 5 0 0 0 10 0h2a7 7 0 0 1-6 6.93V21h-2Z" />
  </svg>
)

export const MicOffIcon = () => (
  <svg {...base}>
    <path d="m19.8 22.6-4.02-4.02A6.97 6.97 0 0 1 13 18.93V21h-2v-2.07A7 7 0 0 1 5 11h2a5 5 0 0 0 7.3 4.44L12.9 14.03A3 3 0 0 1 9 11V10.1L1.4 2.5 2.8 1.1l18.4 18.4-1.4 1.4ZM15 11c0 .34-.06.66-.16.96L9.9 7.02V5a3 3 0 0 1 6 0v6h-.9Zm2.4 3.16-1.46-1.46c.36-.51.62-1.08.78-1.7h2.04a6.96 6.96 0 0 1-1.36 3.16Z" />
  </svg>
)

export const CamIcon = () => (
  <svg {...base}>
    <path d="M4 6h11a1 1 0 0 1 1 1v3.5l4-3.5v10l-4-3.5V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z" />
  </svg>
)

export const CamOffIcon = () => (
  <svg {...base}>
    <path d="m21.4 21.9-4.4-4.4V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h.1L1.6 3.5 3 2.1l19.8 19.8-1.4 1.4v-1.4ZM16 7v3.5l4-3.5v9.17l-9-9H15a1 1 0 0 1 1 1v-1Z" />
  </svg>
)

export const ShareIcon = () => (
  <svg {...base}>
    <path d="M3 5a1 1 0 0 1 1-1h16a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1h-6v2h3v2H7v-2h3v-2H4a1 1 0 0 1-1-1V5Zm2 1v9h14V6H5Zm8 2.5-4 3.2h2.5V15h3v-3.3H17l-4-3.2Z" />
  </svg>
)

export const HandIcon = () => (
  <svg {...base}>
    <path d="M8 12V4.5a1.25 1.25 0 0 1 2.5 0V11h1V3a1.25 1.25 0 0 1 2.5 0v8h1V4.5a1.25 1.25 0 0 1 2.5 0V13h1V8a1.25 1.25 0 0 1 2.5 0v7a7 7 0 0 1-7 7h-.5A7 7 0 0 1 6.5 15v-3a1.25 1.25 0 0 1 1.5-1.22V12Z" />
  </svg>
)

export const EmojiIcon = () => (
  <svg {...base}>
    <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm0 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16ZM8.5 11A1.5 1.5 0 1 0 8.5 8a1.5 1.5 0 0 0 0 3Zm7 0a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm-8.03 3a5.5 5.5 0 0 0 9.06 0H7.47Z" />
  </svg>
)

export const ChatIcon = () => (
  <svg {...base}>
    <path d="M4 3h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H8l-5 4V4a1 1 0 0 1 1-1Zm3 5h10v2H7V8Zm0 4h7v2H7v-2Z" />
  </svg>
)

export const SettingsIcon = () => (
  <svg {...base}>
    <path d="m19.4 13 2.1 1.6-2 3.5-2.5-1a7.6 7.6 0 0 1-1.7 1l-.4 2.6h-4l-.4-2.6a7.6 7.6 0 0 1-1.7-1l-2.5 1-2-3.5L6.4 13a7.7 7.7 0 0 1 0-2L4.3 9.4l2-3.5 2.5 1a7.6 7.6 0 0 1 1.7-1l.4-2.6h4l.4 2.6c.6.26 1.17.6 1.7 1l2.5-1 2 3.5-2.1 1.6a7.7 7.7 0 0 1 0 2ZM12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z" />
  </svg>
)

export const MenuIcon = () => (
  <svg {...base} fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round">
    <path d="M3 6h18M3 12h18M3 18h18" />
  </svg>
)

export const RecordIcon = () => (
  <svg {...base}>
    <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="2" />
    <circle cx="12" cy="12" r="4.5" />
  </svg>
)

export const StopIcon = () => (
  <svg {...base}>
    <rect x="6" y="6" width="12" height="12" rx="2" />
  </svg>
)

export const PeopleIcon = () => (
  <svg {...base}>
    <path d="M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Zm7 1a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm-7 1c-3.05 0-7 1.53-7 4.5V20h14v-2.5c0-2.97-3.95-4.5-7-4.5Zm7 .5c-.47 0-.97.05-1.5.13 1.06.9 1.5 2.06 1.5 3.37V20h6v-2c0-2.3-3.63-3.5-6-3.5Z" />
  </svg>
)

export const HangupIcon = () => (
  <svg {...base}>
    <path d="M12 9c-2.9 0-5.6.66-8 1.85l-2 2.4L4.4 16l3.1-1.5.5-2.7c1.24-.4 2.6-.6 4-.6s2.76.2 4 .6l.5 2.7L19.6 16 22 13.24l-2-2.39A17.9 17.9 0 0 0 12 9Z" />
  </svg>
)

export const DownloadIcon = () => (
  <svg {...base} width={18} height={18}>
    <path d="M12 3v10.17l3.6-3.6L17 11l-5 5-5-5 1.4-1.42L12 13.17V3Zm-7 15h14v2H5v-2Z" />
  </svg>
)

export const CloseIcon = () => (
  <svg {...base} width={18} height={18}>
    <path d="M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19 5 17.6 10.6 12 5 6.4 6.4 5Z" />
  </svg>
)

export const StageIcon = () => (
  <svg {...base}>
    <path d="M3 5h18v2H3V5Zm2 4h14l-1.5 9.5a1 1 0 0 1-1 .5H7.5a1 1 0 0 1-1-.5L5 9Zm7 2a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Z" />
  </svg>
)

export const CubeIcon = () => (
  <svg {...base}>
    <path d="M12 2 3 7v10l9 5 9-5V7l-9-5Zm0 2.3 6.5 3.6L12 11.5 5.5 7.9 12 4.3ZM5 9.6l6 3.3v6.8l-6-3.3V9.6Zm14 0v6.8l-6 3.3v-6.8l6-3.3Z" />
  </svg>
)

export const NoteIcon = () => (
  <svg {...base}>
    <path d="M6 2h9l5 5v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1Zm8 1.5V8h4.5L14 3.5ZM8 12h8v2H8v-2Zm0 4h8v2H8v-2Zm0-8h4v2H8V8Z" />
  </svg>
)

export const ChevronLeftIcon = () => (
  <svg {...base} width={20} height={20}>
    <path d="M15.4 7.4 14 6l-6 6 6 6 1.4-1.4-4.6-4.6 4.6-4.6Z" />
  </svg>
)

export const ChevronRightIcon = () => (
  <svg {...base} width={20} height={20}>
    <path d="M8.6 7.4 10 6l6 6-6 6-1.4-1.4 4.6-4.6-4.6-4.6Z" />
  </svg>
)

export const ClockIcon = () => (
  <svg {...base} width={16} height={16}>
    <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm0 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Zm.5-13H11v6l5 3 .75-1.2-4.25-2.55V7Z" />
  </svg>
)

export const VideoIcon = () => (
  <svg {...base} width={28} height={28}>
    <path d="M4 6h11a1 1 0 0 1 1 1v3.5l4-3.5v10l-4-3.5V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z" />
  </svg>
)

export const ChevronUpIcon = () => (
  <svg {...base} width={14} height={14}>
    <path d="M12 8.3 5.3 15 4 13.6l8-8 8 8L18.7 15 12 8.3Z" />
  </svg>
)

export const VoiceCallIcon = () => (
  <svg {...base}>
    <path d="M6.6 10.8a15.5 15.5 0 0 0 6.6 6.6l2.2-2.2a1 1 0 0 1 1-.24c1.1.37 2.3.57 3.6.57a1 1 0 0 1 1 1V20a1 1 0 0 1-1 1A17 17 0 0 1 3 4a1 1 0 0 1 1-1h3.5a1 1 0 0 1 1 1c0 1.3.2 2.5.57 3.6a1 1 0 0 1-.25 1l-2.2 2.2Z" />
  </svg>
)

export const CalendarIcon = () => (
  <svg {...base}>
    <path d="M7 2v2H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-2V2h-2v2H9V2H7ZM5 9h14v10H5V9Zm2 2v2h2v-2H7Zm4 0v2h2v-2h-2Zm4 0v2h2v-2h-2Z" />
  </svg>
)

export const FilmIcon = () => (
  <svg {...base}>
    <path d="M4 3h16a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Zm1 3v2h2V6H5Zm12 0v2h2V6h-2ZM5 11v2h2v-2H5Zm12 0v2h2v-2h-2ZM5 16v2h2v-2H5Zm12 0v2h2v-2h-2ZM9 6v12h6V6H9Z" />
  </svg>
)

export const HomeIcon = () => (
  <svg {...base}>
    <path d="M12 3 2 12h3v8h5v-6h4v6h5v-8h3L12 3Z" />
  </svg>
)

export const ShareLinkIcon = () => (
  <svg {...base} width={18} height={18}>
    <path d="M18 16a3 3 0 0 0-2.4 1.2l-6.7-3.9a3 3 0 0 0 0-2.6l6.7-3.9a3 3 0 1 0-.9-1.6L8 9.1a3 3 0 1 0 0 5.8l6.7 3.9A3 3 0 1 0 18 16Z" />
  </svg>
)

export const PlusIcon = () => (
  <svg {...base} width={18} height={18}>
    <path d="M11 5v6H5v2h6v6h2v-6h6v-2h-6V5h-2Z" />
  </svg>
)

export const TrashIcon = () => (
  <svg {...base} width={16} height={16}>
    <path d="M7 4V3a1 1 0 0 1 1-1h8a1 1 0 0 1 1 1v1h4v2H3V4h4Zm-1 4h12l-1 12a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1L6 8Z" />
  </svg>
)

export const EditIcon = () => (
  <svg {...base} width={16} height={16}>
    <path d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25ZM20.71 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83Z" />
  </svg>
)

export const BlurIcon = () => (
  <svg {...base}>
    <path d="M12 4a4 4 0 1 1 0 8 4 4 0 0 1 0-8Zm0 10c3.3 0 8 1.66 8 5v1H4v-1c0-3.34 4.7-5 8-5Z" opacity="0.9" />
    <circle cx="4" cy="6" r="1" /><circle cx="20" cy="6" r="1" />
    <circle cx="3" cy="11" r="1" /><circle cx="21" cy="11" r="1" />
    <circle cx="6" cy="3" r="1" /><circle cx="18" cy="3" r="1" />
  </svg>
)

// ---------------------------------------------------------------------------
//  Ícones de CONTROLO (achado 3.2.5 do docs/ux-perf-review.md)
//
//  Estes lugares eram emoji — ⌕ ▶ ⬇ ✓ ⎘ ✎ ▤ ▦ ☰ ▾ ↵ ↻ 🚪 ◐ — a par deste
//  conjunto SVG. Um emoji num botão renderiza diferente por sistema operativo,
//  NÃO herda `currentColor` (fica colorido sobre um botão que muda de cor) e
//  não escala com --ctl-h. O emoji fica onde é CONTEÚDO — as reações da sala —,
//  nunca onde é controlo.
// ---------------------------------------------------------------------------

export const SearchIcon = () => (
  <svg {...base}>
    <path d="M15.5 14h-.79l-.28-.27a6.5 6.5 0 1 0-.7.7l.27.28v.79l5 4.99L20.49 19l-4.99-5Zm-6 0A4.5 4.5 0 1 1 14 9.5 4.5 4.5 0 0 1 9.5 14Z" />
  </svg>
)

export const PlayIcon = () => (
  <svg {...base}>
    <path d="M8 5v14l11-7L8 5Z" />
  </svg>
)

export const CheckIcon = () => (
  <svg {...base}>
    <path d="M9 16.17 4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41L9 16.17Z" />
  </svg>
)

/** Aviso. A consola não usa emoji como controlo — ver lote2.invariantes. */
export const AlertIcon = () => (
  <svg {...base}>
    <path d="M12 2 1 21h22L12 2Zm0 4.53L19.53 19H4.47L12 6.53ZM11 10v5h2v-5h-2Zm0 6v2h2v-2h-2Z" />
  </svg>
)

export const CopyIcon = () => (
  <svg {...base}>
    <path d="M16 1H4a2 2 0 0 0-2 2v14h2V3h12V1Zm3 4H8a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2Zm0 16H8V7h11v14Z" />
  </svg>
)

export const ChevronDownIcon = () => (
  <svg {...base}>
    <path d="M7.4 8.6 12 13.2l4.6-4.6L18 10l-6 6-6-6 1.4-1.4Z" />
  </svg>
)

export const EnterIcon = () => (
  <svg {...base}>
    <path d="M19 7v4H7.83l3.58-3.59L10 6l-6 6 6 6 1.41-1.41L7.83 13H21V7h-2Z" />
  </svg>
)

export const RepeatIcon = () => (
  <svg {...base}>
    <path d="M7 7h10v3l4-4-4-4v3H5v6h2V7Zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4Z" />
  </svg>
)

/** Vista de biblioteca — linhas densas. */
export const RowsIcon = () => (
  <svg {...base}>
    <path d="M3 5h18v3H3V5Zm0 5.5h18v3H3v-3ZM3 16h18v3H3v-3Z" />
  </svg>
)

/** Vista de cartões — grelha. */
export const GridIcon = () => (
  <svg {...base}>
    <path d="M3 3h8v8H3V3Zm10 0h8v8h-8V3ZM3 13h8v8H3v-8Zm10 0h8v8h-8v-8Z" />
  </svg>
)

/** Vista de tabela — cabeçalho mais linhas. */
export const TableIcon = () => (
  <svg {...base}>
    <path d="M3 4h18v4H3V4Zm0 6h5v10H3V10Zm7 0h5v10h-5V10Zm7 0h4v10h-4V10Z" />
  </svg>
)

export const DoorIcon = () => (
  <svg {...base}>
    <path d="M5 3h9a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H5V3Zm3 8.2v1.6h2v-1.6H8ZM16 5h3v14h-3v-2h1V7h-1V5Z" />
  </svg>
)

/** Interruptor de tema — meia-lua, o mesmo registo do ◐ que substitui. */
export const ThemeIcon = () => (
  <svg {...base}>
    <path d="M12 2a10 10 0 1 0 0 20V2Zm0 18a8 8 0 0 1 0-16v16Z" />
    <path d="M12 2a10 10 0 0 1 0 20 10 10 0 0 0 0-20Z" opacity=".35" />
  </svg>
)

// ---------------------------------------------------------------------------
//  Ícones da SALA (R88). Estes lugares eram emoji — 🔒 ⓘ 🏆 🔊 ➤ 📊 ❓ 🛡 💾 —
//  a par do conjunto SVG que já existia ao lado, na mesma barra. As razões são
//  as mesmas do bloco de controlo acima: um emoji renderiza diferente por
//  sistema operativo, NÃO herda `currentColor` e não escala com o resto.
//
//  A regra da casa mantém-se e é a que decide o que fica: o emoji fica onde é
//  CONTEÚDO — as reações da sala, o selector de emoji do chat, os nomes de
//  teclas em <kbd> —, nunca onde é CONTROLO ou rótulo de secção.
// ---------------------------------------------------------------------------

export const LockIcon = () => (
  <svg {...base}>
    <path d="M12 1a5 5 0 0 1 5 5v3h1a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V11a2 2 0 0 1 2-2h1V6a5 5 0 0 1 5-5Zm3 8V6a3 3 0 1 0-6 0v3h6Zm-3 5a2 2 0 0 0-1 3.73V19h2v-1.27A2 2 0 0 0 12 14Z" />
  </svg>
)

export const InfoIcon = () => (
  <svg {...base}>
    <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm1 15h-2v-6h2v6Zm0-8h-2V7h2v2Z" />
  </svg>
)

export const TrophyIcon = () => (
  <svg {...base}>
    <path d="M18 4h3v3a4 4 0 0 1-3.4 3.96A6 6 0 0 1 13 14.92V18h3v2H8v-2h3v-3.08a6 6 0 0 1-4.6-3.96A4 4 0 0 1 3 7V4h3V3h12v1Zm0 2v4.83A2 2 0 0 0 19 7V6h-1ZM6 6H5v1a2 2 0 0 0 1 1.83V6Z" />
  </svg>
)

export const SpeakerIcon = () => (
  <svg {...base}>
    <path d="M4 9h3l5-4v14l-5-4H4V9Zm12.5 3a3.5 3.5 0 0 0-2-3.16v6.32A3.5 3.5 0 0 0 16.5 12Zm-2 6.7a6 6 0 0 0 0-13.4V3.23a8 8 0 0 1 0 17.54V18.7Z" />
  </svg>
)

export const SendIcon = () => (
  <svg {...base}>
    <path d="M3 20.5v-6l9-2.5-9-2.5v-6l18 8.5-18 8.5Z" />
  </svg>
)

export const ChartIcon = () => (
  <svg {...base}>
    <path d="M4 20V10h4v10H4Zm6 0V4h4v16h-4Zm6 0v-7h4v7h-4Z" />
  </svg>
)

export const HelpIcon = () => (
  <svg {...base}>
    <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm1 17h-2v-2h2v2Zm1.8-7.2-.9.92c-.62.62-.9 1.1-.9 2.28h-2v-.5c0-.88.36-1.68.9-2.22l1.24-1.26A1.96 1.96 0 0 0 12 7.5a2 2 0 0 0-2 2H8a4 4 0 1 1 6.8 2.8Z" />
  </svg>
)

export const ShieldIcon = () => (
  <svg {...base}>
    <path d="M12 1 3 5v6c0 5.05 3.84 9.77 9 11 5.16-1.23 9-5.95 9-11V5l-9-4Zm-1.4 14.6L7 12l1.4-1.4 2.2 2.2 4.6-4.6L16.6 9.6l-6 6Z" />
  </svg>
)

export const SaveIcon = () => (
  <svg {...base}>
    <path d="M17 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V7l-4-4Zm-5 16a3 3 0 1 1 0-6 3 3 0 0 1 0 6Zm3-10H6V5h9v4Z" />
  </svg>
)

/** Espessura de traço no quadro branco. Eram `─` e `━`: legíveis, mas um
 *  controlo com um caractere lá dentro herda a fonte e não a cor do botão. */
export const StrokeThinIcon = () => (
  <svg {...base}><rect x="3" y="11" width="18" height="1.5" rx="0.75" /></svg>
)
export const StrokeThickIcon = () => (
  <svg {...base}><rect x="3" y="9.5" width="18" height="5" rx="2.5" /></svg>
)

export const PinIcon = () => (
  <svg {...base}>
    <path d="M14 4V2H6v2h1v6l-2 2v2h5v6l1 2 1-2v-6h5v-2l-2-2V4h1Z" />
  </svg>
)

export const FullscreenIcon = () => (
  <svg {...base}>
    <path d="M4 9V4h5v2H6v3H4Zm11-5h5v5h-2V6h-3V4ZM4 15h2v3h3v2H4v-5Zm14 0h2v5h-5v-2h3v-3Z" />
  </svg>
)
