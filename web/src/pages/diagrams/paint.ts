/**
 * Cores do PAPEL dos diagramas.
 *
 * Porque há hexadecimais aqui e não tokens: o diagrama é um DOCUMENTO, não
 * cromado da interface. Sai para SVG, PNG, XMI e .bpmn e tem de ter o mesmo
 * aspecto fora da aplicação — onde `var(--text)` não existe — e o mesmo
 * aspecto em tema claro e escuro (o template desenha-o sempre em papel claro
 * sobre a consola escura). É o único ficheiro dos quadros com cores escritas;
 * a selecção, os puxadores e o resto da interface usam os tokens.
 *
 * Os valores são os do template (DelonixCanvasUML/BPMN): tinta #0b0b0c sobre
 * branco, grelha #dedee1, tinta secundária #5c5c63, destaque #ad1017 (o
 * `--accent` do tema claro), verde #1e7a4a e âmbar #a85b00 dos eventos.
 */
export const INK = {
  paper: '#f7f7f8',
  grid: '#dedee1',
  surface: '#ffffff',
  header: '#f2f2f3',
  ink: '#0b0b0c',
  muted: '#5c5c63',
  accent: '#ad1017',
  accentTint: '#f6eceb',
  start: '#1e7a4a',
  amber: '#a85b00',
  noteFill: '#fffdf0',
  noteInk: '#4a4326',
} as const

/** Preenchimentos que a pessoa pode escolher no separador «Estilo». */
export const FILLS: Record<string, string> = {
  white: '#ffffff',
  grey: '#f2f2f3',
  yellow: '#fffdf0',
  red: '#f6eceb',
  blue: '#e9eff6',
  green: '#e8f3ec',
}

/** Cores da caneta no separador «Livre». */
export const PENS: Record<string, string> = {
  ink: '#0b0b0c',
  red: '#ad1017',
  blue: '#2f5d8a',
  green: '#1e7a4a',
  amber: '#c77700',
  purple: '#6b3fa0',
  grey: '#8a8a91',
  yellow: '#f2c200',
}

/** Espessuras da caneta (unidades do quadro). */
export const PEN_WIDTHS = [1.5, 2.5, 5, 9] as const

/** Opacidades oferecidas; o marcador usa a sua. */
export const PEN_OPACITIES = [1, 0.6, 0.3] as const
export const MARKER = { width: 16, opacity: 0.35 } as const

/** Cores dos post-its. */
export const STICKY: Record<string, { fill: string; edge: string }> = {
  yellow: { fill: '#fff4a8', edge: '#e0cf5c' },
  pink: { fill: '#ffd6e4', edge: '#e39bb5' },
  blue: { fill: '#d4e8ff', edge: '#8fb6e3' },
  green: { fill: '#d6f2dc', edge: '#8ccb9a' },
  orange: { fill: '#ffe0bf', edge: '#e8aa6a' },
}

/**
 * Convenção de cores do C4 (C4-PlantUML, MIT): pessoa, sistema, contentor e
 * componente em azuis que escurecem com o nível; externos em cinzento.
 */
export const C4 = {
  person: '#08427b',
  personExt: '#686868',
  system: '#1168bd',
  systemExt: '#999999',
  container: '#438dd5',
  containerExt: '#b3b3b3',
  component: '#85bbf0',
  componentExt: '#cccccc',
  code: '#e9f2fb',
  boundary: '#444444',
  node: '#888888',
  text: '#ffffff',
  textDark: '#0b0b0c',
  rel: '#707070',
} as const

export const FONT = "Archivo, 'Helvetica Neue', Arial, sans-serif"
export const MONO = "'DM Mono', ui-monospace, Menlo, monospace"
