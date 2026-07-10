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
