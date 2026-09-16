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
}

export const FONT = "Archivo, 'Helvetica Neue', Arial, sans-serif"
export const MONO = "'DM Mono', ui-monospace, Menlo, monospace"
