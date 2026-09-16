/**
 * O desenho das peças do palco que não são fontes: o cartão de intervalo com
 * a marca e as sobreposições queimadas na imagem (logótipo, rodapé, ticker,
 * cronómetro, sondagem, legendas). Funções sobre um contexto 2D — o
 * compositor chama-as a cada frame com o seu estado.
 */
import { hhmmss, type Sobreposicoes } from './palco'

/** Dados de uma sondagem da sala, para a sobreposição (vêm do servidor tal e qual). */
export interface SondagemNoPalco {
  pergunta: string
  opcoes: { texto: string; votos: number }[]
  aberta: boolean
}

/** O que o cartão de intervalo mostra. Os textos chegam traduzidos da página. */
export interface MarcaDoPalco {
  nome: string
  titulo: string
  aviso: string
  /** `true` com o nome de origem: desenha-se o símbolo; senão, a inicial (R100). */
  deOrigem: boolean
  logo: CanvasImageSource | null
}

/** O símbolo Delonix (o de `ui/icons.tsx`), em caminhos de canvas. */
const SIMBOLO_RAIOS = 'M12 12 15.4 3.9M12 12 20.1 8.6M12 12 20.1 15.4M12 12 15.4 20.1M12 12 3.9 15.4M12 12 3.9 8.6'
/** Criado no primeiro uso: `Path2D` não existe em Node, onde os testes importam isto. */
let raios: Path2D | null = null
const SIMBOLO_PONTOS: [number, number][] = [
  [16.2, 3],
  [21, 7.8],
  [21, 16.2],
  [16.2, 21],
  [3, 16.2],
  [3, 7.8],
]

/** O cartão de intervalo: a marca, o título da sessão e o aviso. */
export function desenharCartaoDeMarca(c: CanvasRenderingContext2D, marca: MarcaDoPalco, W: number, H: number): void {
  const g = c.createLinearGradient(0, 0, W, H)
  g.addColorStop(0, '#2a0a0d')
  g.addColorStop(1, '#0d1117')
  c.fillStyle = g
  c.fillRect(0, 0, W, H)
  const s = H / 1080
  const lado = 150 * s
  desenharSimbolo(c, marca, W / 2 - lado / 2, H * 0.3 - lado / 2, lado, '#e8232b')
  c.textAlign = 'center'
  c.fillStyle = '#ffffff'
  c.font = `700 ${Math.round(64 * s)}px Archivo, system-ui, sans-serif`
  if (marca.nome) c.fillText(marca.nome, W / 2, H * 0.52, W * 0.8)
  c.font = `500 ${Math.round(40 * s)}px Archivo, system-ui, sans-serif`
  c.fillStyle = 'rgba(255,255,255,0.8)'
  if (marca.titulo) c.fillText(marca.titulo, W / 2, H * 0.6, W * 0.8)
  c.font = `500 ${Math.round(30 * s)}px "DM Mono", ui-monospace, monospace`
  c.fillStyle = '#f0a32e'
  if (marca.aviso) c.fillText(marca.aviso.toUpperCase(), W / 2, H * 0.72, W * 0.8)
  c.textAlign = 'start'
}

/** O logótipo carregado, o símbolo Delonix, ou a inicial (instância renomeada). */
export function desenharSimbolo(
  c: CanvasRenderingContext2D,
  marca: MarcaDoPalco,
  x: number,
  y: number,
  lado: number,
  cor: string,
): void {
  if (marca.logo) {
    c.drawImage(marca.logo, x, y, lado, lado)
    return
  }
  if (!marca.deOrigem) {
    c.fillStyle = cor
    c.beginPath()
    c.roundRect(x, y, lado, lado, lado * 0.18)
    c.fill()
    c.fillStyle = '#ffffff'
    c.font = `700 ${Math.round(lado * 0.55)}px Archivo, system-ui, sans-serif`
    c.textAlign = 'center'
    c.textBaseline = 'middle'
    c.fillText((marca.nome.trim().charAt(0) || '?').toUpperCase(), x + lado / 2, y + lado / 2)
    c.textAlign = 'start'
    c.textBaseline = 'alphabetic'
    return
  }
  c.save()
  c.translate(x, y)
  c.scale(lado / 24, lado / 24)
  c.strokeStyle = cor
  c.fillStyle = cor
  c.lineWidth = 1.5
  c.lineCap = 'round'
  c.beginPath()
  c.arc(12, 12, 3, 0, Math.PI * 2)
  c.fill()
  for (const r of [6, 8.7]) {
    c.beginPath()
    c.arc(12, 12, r, 0, Math.PI * 2)
    c.stroke()
  }
  raios ??= new Path2D(SIMBOLO_RAIOS)
  c.stroke(raios)
  for (const [px, py] of SIMBOLO_PONTOS) {
    c.beginPath()
    c.arc(px, py, 1.4, 0, Math.PI * 2)
    c.fill()
  }
  c.restore()
}

/** O estado que as sobreposições lêem a cada frame. */
export interface EstadoDasSobreposicoes {
  sobreposicoes: Sobreposicoes
  rodapeDesde: number
  cronometroDesde: number
  legenda: string
  sondagem: SondagemNoPalco | null
  marca: MarcaDoPalco
}

/**
 * As sobreposições QUEIMADAS na imagem — as que o espectador vê. Tudo à
 * escala da altura (os números são para 1080 linhas), para o 4K sair igual.
 */
export function desenharSobreposicoes(
  c: CanvasRenderingContext2D,
  e: EstadoDasSobreposicoes,
  W: number,
  H: number,
): void {
  const o = e.sobreposicoes
  const s = H / 1080
  const margem = 36 * s
  const agora = Date.now()

  if (o.logotipo && (e.marca.nome || e.marca.logo || e.marca.deOrigem)) {
    const lado = 34 * s
    c.font = `600 ${Math.round(24 * s)}px Archivo, system-ui, sans-serif`
    const texto = e.marca.nome
    const largura = lado + (texto ? c.measureText(texto).width + 30 * s : 16 * s) + 12 * s
    c.fillStyle = 'rgba(0,0,0,0.62)'
    c.beginPath()
    c.roundRect(margem, margem, largura, lado + 18 * s, 8 * s)
    c.fill()
    desenharSimbolo(c, e.marca, margem + 9 * s, margem + 9 * s, lado, '#e8232b')
    if (texto) {
      c.fillStyle = '#ffffff'
      c.fillText(texto, margem + lado + 22 * s, margem + 9 * s + lado * 0.74)
    }
  }

  if (o.cronometro && e.cronometroDesde) {
    const passados = (agora - e.cronometroDesde) / 1000
    const valor = o.minutos > 0 ? Math.max(0, o.minutos * 60 - passados) : passados
    const texto = hhmmss(valor)
    c.font = `500 ${Math.round(34 * s)}px "DM Mono", ui-monospace, monospace`
    const largura = c.measureText(texto).width + 32 * s
    const x = W - margem - largura
    c.fillStyle = o.minutos > 0 && valor <= 60 ? 'rgba(217,45,32,0.9)' : 'rgba(0,0,0,0.62)'
    c.beginPath()
    c.roundRect(x, margem, largura, 54 * s, 8 * s)
    c.fill()
    c.fillStyle = '#ffffff'
    c.fillText(texto, x + 16 * s, margem + 39 * s)
  }

  if (o.sondagem && e.sondagem && e.sondagem.opcoes.length) {
    const sd = e.sondagem
    const largura = 520 * s
    const linha = 58 * s
    const altura = 86 * s + sd.opcoes.length * linha
    const x = W - margem - largura
    const y = Math.max(margem + 70 * s, H * 0.18)
    const total = sd.opcoes.reduce((n, op) => n + op.votos, 0)
    c.fillStyle = 'rgba(13,17,23,0.86)'
    c.beginPath()
    c.roundRect(x, y, largura, altura, 12 * s)
    c.fill()
    c.fillStyle = '#ffffff'
    c.font = `700 ${Math.round(28 * s)}px Archivo, system-ui, sans-serif`
    c.fillText(sd.pergunta, x + 22 * s, y + 48 * s, largura - 44 * s)
    sd.opcoes.forEach((op, i) => {
      const oy = y + 72 * s + i * linha
      const pct = total ? op.votos / total : 0
      c.fillStyle = 'rgba(255,255,255,0.12)'
      c.fillRect(x + 22 * s, oy, largura - 44 * s, 42 * s)
      c.fillStyle = 'rgba(232,35,43,0.75)'
      c.fillRect(x + 22 * s, oy, (largura - 44 * s) * pct, 42 * s)
      c.fillStyle = '#ffffff'
      c.font = `500 ${Math.round(22 * s)}px Archivo, system-ui, sans-serif`
      c.fillText(op.texto, x + 34 * s, oy + 29 * s, largura - 170 * s)
      c.font = `500 ${Math.round(22 * s)}px "DM Mono", ui-monospace, monospace`
      c.textAlign = 'end'
      c.fillText(`${Math.round(pct * 100)}%`, x + largura - 34 * s, oy + 29 * s)
      c.textAlign = 'start'
    })
  }

  const baseRodape = H - margem
  if (o.rodape && (o.nome || o.cargo)) {
    // Entrada animada: desliza da esquerda em 600 ms, com saída suave.
    const p = Math.min(1, (agora - e.rodapeDesde) / 600)
    const entrada = 1 - Math.pow(1 - p, 3)
    c.font = `700 ${Math.round(38 * s)}px Archivo, system-ui, sans-serif`
    const wNome = c.measureText(o.nome).width
    c.font = `500 ${Math.round(26 * s)}px Archivo, system-ui, sans-serif`
    const wCargo = c.measureText(o.cargo).width
    const largura = Math.max(wNome, wCargo) + 60 * s
    const altura = (o.cargo ? 104 : 70) * s
    const x = margem - (1 - entrada) * (largura + margem)
    const y = baseRodape - altura - (o.legendas && e.legenda ? 120 * s : 0)
    c.globalAlpha = entrada
    c.fillStyle = 'rgba(13,17,23,0.88)'
    c.fillRect(x, y, largura, altura)
    c.fillStyle = '#e8232b'
    c.fillRect(x, y, 8 * s, altura)
    c.fillStyle = '#ffffff'
    c.font = `700 ${Math.round(38 * s)}px Archivo, system-ui, sans-serif`
    c.fillText(o.nome, x + 30 * s, y + 48 * s)
    if (o.cargo) {
      c.fillStyle = 'rgba(255,255,255,0.78)'
      c.font = `500 ${Math.round(26 * s)}px Archivo, system-ui, sans-serif`
      c.fillText(o.cargo, x + 30 * s, y + 86 * s)
    }
    c.globalAlpha = 1
  }

  if (o.ticker && o.url) {
    c.font = `500 ${Math.round(26 * s)}px "DM Mono", ui-monospace, monospace`
    const largura = c.measureText(o.url).width + 32 * s
    const x = W - margem - largura
    const y = baseRodape - 46 * s - (o.legendas && e.legenda ? 120 * s : 0)
    c.fillStyle = 'rgba(0,0,0,0.62)'
    c.fillRect(x, y, largura, 46 * s)
    c.fillStyle = '#ffffff'
    c.fillText(o.url, x + 16 * s, y + 32 * s)
  }

  if (o.legendas && e.legenda) {
    c.font = `600 ${Math.round(40 * s)}px Archivo, system-ui, sans-serif`
    const linhas = partirEmLinhas(e.legenda, (t) => c.measureText(t).width, W * 0.72).slice(-2)
    const alturaLinha = 52 * s
    const altura = linhas.length * alturaLinha + 24 * s
    const largura = Math.max(...linhas.map((l) => c.measureText(l).width)) + 48 * s
    const x = (W - largura) / 2
    const y = baseRodape - altura
    c.fillStyle = 'rgba(0,0,0,0.78)'
    c.fillRect(x, y, largura, altura)
    c.fillStyle = '#ffffff'
    c.textAlign = 'center'
    linhas.forEach((l, i) => c.fillText(l, W / 2, y + 12 * s + (i + 1) * alturaLinha - 14 * s))
    c.textAlign = 'start'
  }
}

/** Parte um texto em linhas que caibam numa largura (medida pelo canvas). */
export function partirEmLinhas(texto: string, medir: (t: string) => number, largura: number): string[] {
  const linhas: string[] = []
  let actual = ''
  for (const palavra of texto.split(/\s+/).filter(Boolean)) {
    const tentativa = actual ? `${actual} ${palavra}` : palavra
    if (actual && medir(tentativa) > largura) {
      linhas.push(actual)
      actual = palavra
    } else {
      actual = tentativa
    }
  }
  if (actual) linhas.push(actual)
  return linhas
}
