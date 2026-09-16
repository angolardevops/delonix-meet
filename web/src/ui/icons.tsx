/**
 * Ícones de traço (24×24, 1.6 px). O template usa glifos unicode como
 * esboço; na app são SVG para terem a mesma espessura em todos os sistemas
 * e herdarem `currentColor`. Um ícone novo é uma entrada no mapa — não um
 * `<svg>` escrito dentro de uma página.
 */
import type { SVGProps } from 'react'

const P: Record<string, string> = {
  home: 'M3 10.5 12 3l9 7.5M5 9v11h5v-6h4v6h5V9',
  calendar: 'M4 6h16v14H4zM4 10h16M8 3v4M16 3v4',
  video: 'M3 7h12v10H3zM15 10.5 21 7v10l-6-3.5',
  videoOff: 'M3 7h12v10H3zM15 10.5 21 7v10l-6-3.5M2 3l20 18',
  mic: 'M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3',
  micOff: 'M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3M3 3l18 18',
  screen: 'M3 4h18v12H3zM8 20h8M12 16v4M9 10l3-3 3 3M12 7v6',
  hand: 'M8 13V5.5a1.5 1.5 0 0 1 3 0V11M11 10V4.5a1.5 1.5 0 0 1 3 0V11M14 10.5V6a1.5 1.5 0 0 1 3 0v8a7 7 0 0 1-7 7h-.5A6.5 6.5 0 0 1 4 16.5L3.2 14a1.5 1.5 0 0 1 2.6-1.4L8 15',
  chat: 'M4 5h16v11H9l-5 4z',
  people: 'M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM2.5 20a6.5 6.5 0 0 1 13 0M16 4.5a3.5 3.5 0 0 1 0 6.5M18 14a6 6 0 0 1 3.5 6',
  more: 'M5 12h.01M12 12h.01M19 12h.01',
  phoneOff: 'M4 14.5c4.5-4 11.5-4 16 0l-2 3-3.5-1.2V14c-2-.7-3-.7-5 0v2.3L6 17.5z',
  phone: 'M5 3h4l2 5-2.5 1.5a11 11 0 0 0 6 6L16 13l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 3 5a2 2 0 0 1 2-2z',
  record: 'M12 5a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z',
  live: 'M7 7a7 7 0 0 0 0 10M17 7a7 7 0 0 1 0 10M4 4a11 11 0 0 0 0 16M20 4a11 11 0 0 1 0 16M12 10.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z',
  check: 'M4.5 12.5 9.5 17.5 19.5 6.5',
  x: 'M6 6l12 12M18 6 6 18',
  plus: 'M12 5v14M5 12h14',
  minus: 'M5 12h14',
  search: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5 21 21',
  chevronDown: 'M6 9l6 6 6-6',
  chevronUp: 'M6 15l6-6 6 6',
  chevronLeft: 'M15 6l-6 6 6 6',
  chevronRight: 'M9 6l6 6-6 6',
  copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
  link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
  download: 'M12 4v11M7 10l5 5 5-5M4 20h16',
  upload: 'M12 20V9M7 14l5-5 5 5M4 4h16',
  play: 'M7 4.5v15l12-7.5z',
  pause: 'M8 5v14M16 5v14',
  trash: 'M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13',
  edit: 'M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4',
  bell: 'M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0',
  globe: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9z',
  sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4',
  moon: 'M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z',
  logout: 'M10 4H5v16h5M15 8l4 4-4 4M19 12H9',
  lock: 'M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3',
  shield: 'M12 3 4.5 6v5.5c0 4.5 3.2 8 7.5 9.5 4.3-1.5 7.5-5 7.5-9.5V6z',
  shieldCheck: 'M12 3 4.5 6v5.5c0 4.5 3.2 8 7.5 9.5 4.3-1.5 7.5-5 7.5-9.5V6zM9 12l2 2 4-4',
  key: 'M14.5 3.5a5 5 0 1 0 3.5 8.6L21 15v3h-3v2h-3v-2.5l-2.4-2.4A5 5 0 0 0 14.5 3.5zM15.5 7.5h.01',
  alert: 'M12 4 2.5 20h19zM12 10v4M12 17h.01',
  info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5M12 8h.01',
  clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2',
  sparkles: 'M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8zM19 15l.8 2.2 2.2.8-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z',
  grid: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
  speaker: 'M3 4h13v11H3zM18 6h3v4h-3zM18 12h3v4h-3zM3 17h13v3H3z',
  pin: 'M9 3h6l-1 6 3 3H7l3-3zM12 12v9',
  send: 'M4 12 20 4l-6 16-3-7z',
  paperclip: 'M20 11.5 12 19.5a5 5 0 0 1-7-7l8.5-8.5a3.5 3.5 0 0 1 5 5L10 17.5a2 2 0 0 1-3-3l7.5-7.5',
  smile: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8.5 14a4.5 4.5 0 0 0 7 0M9 9.5h.01M15 9.5h.01',
  captions: 'M3 5h18v14H3zM7 10.5a2 2 0 1 0 0 3M13 10.5a2 2 0 1 0 0 3',
  pip: 'M3 5h18v14H3zM12 12h7v5h-7z',
  maximize: 'M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5',
  volume: 'M4 9h4l5-4v14l-5-4H4zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12',
  signal: 'M4 20v-3M9 20v-7M14 20V9M19 20V4',
  external: 'M14 4h6v6M20 4l-9 9M18 14v6H4V6h6',
  eye: 'M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
  layers: 'M12 3 2 8l10 5 10-5zM2 13l10 5 10-5M2 17.5l10 5 10-5',
  sliders: 'M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0M14 4v4M8 10v4M16 16v4',
  board: 'M3 4h18v12H3zM8 20l4-4 4 4M7 8h6M7 11h9',
  film: 'M3 4h18v16H3zM7 4v16M17 4v16M3 8h4M3 12h4M3 16h4M17 8h4M17 12h4M17 16h4',
  plug: 'M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4',
  server: 'M4 4h16v6H4zM4 14h16v6H4zM8 7h.01M8 17h.01',
  chart: 'M4 20V4M4 20h16M8 16v-5M12 16V8M16 16v-3',
  building: 'M5 21V4h10v17M15 9h4v12M8 7h4M8 11h4M8 15h4M3 21h18',
  user: 'M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0',
  userPlus: 'M10 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM2 21a8 8 0 0 1 14.5-4.6M19 14v6M16 17h6',
  door: 'M5 21V3h11v18M16 5h3v16M12 12h.01M3 21h18',
  keyboard: 'M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
  refresh: 'M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5',
  share: 'M16 5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM8 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM16 14a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM10.2 11l3.6-2M10.2 13l3.6 2',
  filter: 'M3 5h18l-7 8v6l-4 2v-8z',
  menu: 'M4 6h16M4 12h16M4 18h16',
  poll: 'M4 20h16M6 16V10M11 16V5M16 16v-8',
  question: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17h.01',
  pen: 'M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3',
  eraser: 'M8 20h12M5 15l8-8 6 6-6 6H9z',
  text: 'M5 6V4h14v2M12 4v16M9 20h6',
  square: 'M5 5h14v14H5z',
  arrow: 'M5 19 19 5M10 5h9v9',
  scissors: 'M6 7a2.5 2.5 0 1 0 0-.01zM6 17a2.5 2.5 0 1 0 0 .01zM8 8l12 10M8 16 20 6',
  wand: 'M4 20 15 9M14 4v3M19.5 5.5l-2 2M20 10h-3M9 4.5l1.5 1.5',
  cpu: 'M7 7h10v10H7zM10 10h4v4h-4zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4',
  database: 'M12 3c4.4 0 8 1.3 8 3s-3.6 3-8 3-8-1.3-8-3 3.6-3 8-3zM4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3',
  ban: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM5.6 5.6l12.8 12.8',
  wifi: 'M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01',
  // auth-publico
  eyeOff: 'M2 12s3.5-7 10-7c1.6 0 3 .4 4.2 1M22 12s-3.5 7-10 7c-1.6 0-3-.4-4.2-1M9.9 9.9a3 3 0 0 0 4.2 4.2M3 3l18 18',
  // home-agenda
  repeat: 'M4 11V9a3 3 0 0 1 3-3h12M16 3l3 3-3 3M20 13v2a3 3 0 0 1-3 3H5M8 21l-3-3 3-3',
  list: 'M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01',
  // sala
  trophy: 'M8 4h8v5a4 4 0 0 1-8 0zM8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M10 17h4v3h-4z',
  thumbUp: 'M7 11v9H4v-9zM7 11l4-7a2 2 0 0 1 3 1.7L13.3 10H19a2 2 0 0 1 2 2.3l-1.2 6A2 2 0 0 1 17.8 20H7',
  stop: 'M6 6h12v12H6z',
  blur: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 3v18M16 5.5v13M19.5 9v6',
  cube: 'M12 3 20 7.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12 4 7.5',
  rows: 'M4 4h16v9H4zM4 16h4v4H4zM10 16h4v4h-4zM16 16h4v4h-4z',
  columns: 'M3 4h13v16H3zM18 4h3v4h-3zM18 10h3v4h-3zM18 16h3v4h-3z',
  bot: 'M6 8h12v11H6zM12 4v4M9.5 13h.01M14.5 13h.01M9 16h6M3 12v3M21 12v3',
  notes: 'M5 3h14v18H5zM8 8h8M8 12h8M8 16h5',
  hourglass: 'M7 3h10M7 21h10M8 3c0 5 8 5 8 9s-8 4-8 9M16 3c0 5-8 5-8 9',
  undo: 'M4 9h11a5 5 0 0 1 0 10H9M8 5 4 9l4 4',
  // sala · quadro (frontend/l1-sala)
  circle: 'M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16z',
  arrowRight: 'M4 12h15M14 7l5 5-5 5',
  highlighter: 'M4 20h16M7 16l9.5-10 3.5 3.5L10 19H7z',
  shapes: 'M4 4h7v7H4zM17.5 13a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7zM16 4l4 7h-8z',
  ruler: 'M3 9h18v6H3zM7 9v3M11 9v3M15 9v3M19 9v2',
  // sala · lote 2 (frontend/l2-sala)
  arrowUp: 'M12 19V5M6 11l6-6 6 6',
  cursor: 'M5 3l14 7-6 2-2 6z',
  laser: 'M12 3v4M12 17v4M3 12h4M17 12h4M12 12h.01',
  note: 'M5 4h14v11l-5 5H5zM14 20v-5h5',
  move: 'M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3',
}

export type IconName = keyof typeof P

export function Icon({
  name,
  size,
  ...rest
}: { name: IconName; size?: number } & Omit<SVGProps<SVGSVGElement>, 'name'>) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="dx-icon"
      width={size}
      height={size}
      style={size ? { width: size, height: size } : undefined}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      <path d={P[name]} />
    </svg>
  )
}

/**
 * Símbolo Delonix: flor de Delonix como antenas de rede, anéis de sinal e
 * o mundo ao centro. Desenho exacto do template (ecrã «Sistema de design»).
 */
export function DelonixSymbol({ size = 26, title }: { size?: number; title?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <circle cx="12" cy="12" r="3" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="6" />
      <circle cx="12" cy="12" r="8.7" />
      <path d="M12 12 15.4 3.9M12 12 20.1 8.6M12 12 20.1 15.4M12 12 15.4 20.1M12 12 3.9 15.4M12 12 3.9 8.6" />
      <g fill="currentColor" stroke="none">
        <circle cx="16.2" cy="3" r="1.4" />
        <circle cx="21" cy="7.8" r="1.4" />
        <circle cx="21" cy="16.2" r="1.4" />
        <circle cx="16.2" cy="21" r="1.4" />
        <circle cx="3" cy="16.2" r="1.4" />
        <circle cx="3" cy="7.8" r="1.4" />
      </g>
    </svg>
  )
}
