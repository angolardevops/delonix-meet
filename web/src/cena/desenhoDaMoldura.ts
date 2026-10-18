/**
 * O DESENHO da moldura de transmissão — o irmão impuro do `moldura.ts`, pela
 * mesma separação que o `studio/tv/mesa.ts` e o `desenhoDaMesa.ts` já usam:
 * ali a geometria, aqui os píxeis.
 *
 * Mas «impuro» não quer dizer «não testável». Sem jsdom neste repo, uma função
 * que receba um `CanvasRenderingContext2D` não se chama em Node — por isso tudo
 * o que é DECIDÍVEL antes de tocar no contexto sai daqui como função pura:
 * `enquadrar` (o `cover`) e `posicoesDaFaixa` (o mosaico). O que fica por
 * testar é só a sequência de chamadas ao contexto, e essa prova-se no browser,
 * por amostragem de píxeis, como o `e2e/estudio-palco.mjs` já faz.
 *
 * As cores daqui são de IMAGEM, não de interface: vão para o vídeo como píxeis.
 * Por isso são literais e não tokens — o tema escuro não pode mudar a cor de
 * uma emissão já gravada, nem a de uma que esteja no ar.
 */
import {
  deslocamentoDaFaixa,
  rectsDaMoldura,
  type OpcoesDaMoldura,
  type Rect,
  type RectsDaMoldura,
} from './moldura'

/** Fundo da cena e da moldura. */
export const COR_DO_FUNDO = '#000000'
/** Fundo de um painel que ainda não tem imagem (uma câmara a ligar). */
export const COR_DO_VAZIO = '#101317'
/**
 * A cor da faixa e da assinatura quando quem chama não escolhe outra.
 *
 * É o vermelho da marca (`#e8232b`, o mesmo que o `studio/desenho.ts` usa no
 * símbolo e no rodapé), e é uma cor de IMAGEM: vai para o vídeo como píxeis.
 * Por isso é literal e não token — o tema escuro não pode mudar a cor de uma
 * emissão que está no ar.
 */
export const COR_DA_MARCA = '#e8232b'
/** Tipografia da faixa e da assinatura: monoespaçada, como a referência. */
export const TIPO_MONO = '"DM Mono", ui-monospace, monospace'

/**
 * A escala de desenho: os tamanhos escrevem-se para 1080 linhas e multiplicam-se
 * por isto. É a convenção que o `studio/desenho.ts` já usa (`const s = H / 1080`)
 * e que aqui ganha nome, para que um `grep` encontre as duas pontas — o próprio
 * repo tem a lição escrita no `palco.ts`, sobre o quarto formatador de bytes que
 * escapou à medição por ter nome em português.
 */
export function escalaDaAltura(altura: number): number {
  return Number.isFinite(altura) && altura > 0 ? altura / 1080 : 0
}

/**
 * O rectângulo da fonte, ampliado para COBRIR o destino e centrado — o excesso
 * corta-se. É o `cover` do CSS.
 *
 * Esta conta existe hoje TRÊS vezes no frontend, todas embutidas no meio de um
 * `drawImage` e nenhuma testada: `studio/compositor.ts`, o bloco solto do
 * `room/compositor.ts`, e o `desenhoDaMesa.ts`. Aqui sai como função pura
 * porque é a única forma de a provar sem browser. Como `cena/` é folha, os
 * outros três podem passar a chamá-la — o que não se faz nesta fatia, mas é o
 * caminho para não haver uma quarta cópia.
 *
 * Devolve um rect de área zero quando não há o que enquadrar, em vez de `NaN`:
 * um `NaN` no `drawImage` apaga o frame inteiro sem erro nenhum.
 */
export function enquadrar(fonteW: number, fonteH: number, destino: Rect): Rect {
  const ok =
    Number.isFinite(fonteW) && Number.isFinite(fonteH) && fonteW > 0 && fonteH > 0 && destino.w > 0 && destino.h > 0
  if (!ok) return { x: destino.x, y: destino.y, w: 0, h: 0 }
  const escala = Math.max(destino.w / fonteW, destino.h / fonteH)
  const w = fonteW * escala
  const h = fonteH * escala
  return { x: destino.x + (destino.w - w) / 2, y: destino.y + (destino.h - h) / 2, w, h }
}

/**
 * Quantas repetições da faixa se desenham, no máximo, num frame.
 *
 * Isto é um TECTO DE TRABALHO, não uma preferência de desenho. O número de
 * repetições é `largura / passo`, e o `passo` vem de uma medição de texto feita
 * no contexto — logo é entrada que este módulo não controla. Um passo positivo
 * mas minúsculo (uma fonte que ainda não carregou e mede quase zero) dava um
 * `Math.ceil` enorme e um array gigante alocado a CADA frame, dentro do
 * `requestAnimationFrame` de um directo. Uma faixa com mais de 512 repetições
 * visíveis já não se lê de qualquer maneira.
 */
export const MAXIMO_DE_REPETICOES = 512

/**
 * O irmão do `enquadrar`: a fonte CABE inteira no destino, centrada, com
 * barras onde o rácio não bate certo. É o `contain` do CSS.
 *
 * Existe para o herói da moldura quando ele é uma partilha de ecrã. Cortar um
 * ecrã para encher o rectângulo perde a primeira e a última coluna de texto
 * duns slides — medido: um 16:9 num herói de rácio 1,376 perdia 11% de cada
 * lado, e um 21:9 perdia 41%. Para câmaras continua a valer o `enquadrar`:
 * um retrato com barras pretas é feio sem ser útil.
 */
export function caber(fonteW: number, fonteH: number, destino: Rect): Rect {
  const ok =
    Number.isFinite(fonteW) && Number.isFinite(fonteH) && fonteW > 0 && fonteH > 0 && destino.w > 0 && destino.h > 0
  if (!ok) return { x: destino.x, y: destino.y, w: 0, h: 0 }
  const escala = Math.min(destino.w / fonteW, destino.h / fonteH)
  const w = fonteW * escala
  const h = fonteH * escala
  return { x: destino.x + (destino.w - w) / 2, y: destino.y + (destino.h - h) / 2, w, h }
}

/**
 * Os `x` onde cada repetição do padrão da faixa começa, já deslocados.
 *
 * A faixa é um padrão repetido que anda para a esquerda. Desenha-se uma
 * repetição a mais de cada lado para que não apareça um bordo a entrar ou a
 * sair no meio da animação.
 *
 * As guardas de `isFinite` são de INTENÇÃO, e vale a pena ser franco sobre
 * isso: medido, tirá-las não muda o resultado, porque `Array.from({length: NaN})`
 * e `{length: -1}` já dão lista vazia por coerção. Ficam porque a próxima pessoa
 * a mexer no cálculo de `quantas` não deve ter de redescobrir que a segurança
 * vinha de um acidente do `Array.from`. O que MUDA o comportamento é o tecto
 * acima — e esse tem teste.
 */
export function posicoesDaFaixa(deslocamento: number, passo: number, largura: number): number[] {
  if (!Number.isFinite(passo) || passo <= 0) return []
  if (!Number.isFinite(largura) || largura <= 0) return []
  if (!Number.isFinite(deslocamento)) return []
  const quantas = Math.min(Math.ceil(largura / passo) + 2, MAXIMO_DE_REPETICOES)
  const inicio = -deslocamento - passo
  return Array.from({ length: quantas }, (_, i) => inicio + i * passo)
}

/** O que a moldura mostra. As imagens são o que o compositor já tem em mão. */
export interface FontesDaMoldura {
  /** O conteúdo grande: o ecrã partilhado, ou a câmara quando não há ecrã. */
  heroi: CanvasImageSource | null
  /** As câmaras da coluna, pela ordem em que aparecem. */
  camaras: CanvasImageSource[]
}

/** A marca: o que o operador carregou e escreveu, nada vindo do servidor. */
export interface MarcaDaMoldura {
  logo: CanvasImageSource | null
  /** O texto que a faixa do topo repete. */
  faixa: string
  /** A assinatura na base da coluna. */
  assinatura: string
  /** A cor de destaque da faixa e da assinatura. */
  cor: string
}

/** As dimensões intrínsecas de uma fonte de imagem, para o `cover`. */
function tamanhoDe(fonte: CanvasImageSource): { w: number; h: number } {
  const f = fonte as { videoWidth?: number; videoHeight?: number; width?: number; height?: number }
  // Um `<video>` traz `videoWidth`; uma imagem ou um canvas trazem `width`.
  // Os dois podem ser 0 enquanto a media não chegou — o `enquadrar` trata.
  return { w: f.videoWidth || Number(f.width) || 0, h: f.videoHeight || Number(f.height) || 0 }
}

/** Desenha uma fonte a cobrir o rect, recortada; se não houver, pinta o vazio. */
function pintar(c: CanvasRenderingContext2D, fonte: CanvasImageSource | null, r: Rect): void {
  if (r.w <= 0 || r.h <= 0) return
  if (!fonte) {
    c.fillStyle = COR_DO_VAZIO
    c.fillRect(r.x, r.y, r.w, r.h)
    return
  }
  const { w, h } = tamanhoDe(fonte)
  const d = enquadrar(w, h, r)
  if (d.w <= 0 || d.h <= 0) {
    c.fillStyle = COR_DO_VAZIO
    c.fillRect(r.x, r.y, r.w, r.h)
    return
  }
  c.save()
  c.beginPath()
  c.rect(r.x, r.y, r.w, r.h)
  c.clip()
  c.drawImage(fonte, d.x, d.y, d.w, d.h)
  c.restore()
}

/** A faixa do topo: o texto da marca repetido, a andar para a esquerda. */
export function desenharFaixa(
  c: CanvasRenderingContext2D,
  r: Rect,
  texto: string,
  cor: string,
  instanteMs: number,
  largura: number,
): void {
  if (r.w <= 0 || r.h <= 0) return
  c.fillStyle = COR_DO_FUNDO
  c.fillRect(r.x, r.y, r.w, r.h)
  const limpo = texto.trim()
  if (!limpo) return

  c.save()
  c.beginPath()
  c.rect(r.x, r.y, r.w, r.h)
  c.clip()
  c.font = `500 ${Math.round(r.h * 0.42)}px ${TIPO_MONO}`
  c.textBaseline = 'middle'
  c.fillStyle = cor
  // O passo é o texto mais um vão do tamanho de três espaços — medido no
  // contexto, porque só ele sabe a largura real da fonte carregada.
  const passo = c.measureText(limpo).width + c.measureText('   ').width
  const d = deslocamentoDaFaixa(instanteMs, passo, largura)
  const meio = r.y + r.h / 2
  for (const x of posicoesDaFaixa(d, passo, r.w)) c.fillText(limpo, r.x + x, meio)
  c.restore()
}

/**
 * A MARCA da moldura — faixa, logótipo e assinatura — sobre rects já
 * calculados. NÃO desenha o fundo nem as fontes.
 *
 * É esta a função que os compositores chamam, e não a `desenharMoldura`
 * completa, por uma razão concreta: cada um já sabe desenhar as SUAS fontes de
 * maneiras que esta pasta não conhece nem deve conhecer. O do Estúdio aplica o
 * recorte que o operador escolheu no ecrã (`desenharEcraEm`), espelha a câmara
 * e compõe o avatar com alfa; o da Sala recorta e põe o nome por baixo. Se a
 * cena pintasse as fontes, perdia-se tudo isso — e a cena passaria a precisar
 * de saber o que é um recorte, um avatar e um nome de participante, que é
 * exactamente o acoplamento que a torna folha hoje.
 *
 * A ordem de chamada é: fundo → fontes nos rects → esta função.
 */
export function desenharMarcaDaMoldura(
  c: CanvasRenderingContext2D,
  r: RectsDaMoldura,
  marca: MarcaDaMoldura,
  instanteMs: number,
  largura: number,
  altura: number,
): void {
  const s = escalaDaAltura(altura)

  if (r.logo) {
    if (marca.logo) {
      const { w, h } = tamanhoDe(marca.logo)
      // O logótipo é o único que NÃO é `cover`: cabe inteiro (`contain`), senão
      // uma marca larga sairia cortada ao meio — que é precisamente o que uma
      // marca não pode sofrer.
      const escala = Math.min(r.logo.w / (w || 1), r.logo.h / (h || 1))
      if (w > 0 && h > 0 && Number.isFinite(escala)) {
        const dw = w * escala
        const dh = h * escala
        c.drawImage(marca.logo, r.logo.x + (r.logo.w - dw) / 2, r.logo.y + (r.logo.h - dh) / 2, dw, dh)
      }
    }
    // Sem logótipo não se desenha NADA — fica o fundo. Um rectângulo cinzento
    // no sítio da marca lê-se como uma imagem que falhou a carregar, e a Sala
    // nunca tem logótipo (só o Estúdio o deixa carregar): ia ficar com uma
    // caixa morta no cimo da coluna em todas as emissões.
  }

  if (r.assinatura && marca.assinatura.trim()) {
    c.save()
    c.beginPath()
    c.rect(r.assinatura.x, r.assinatura.y, r.assinatura.w, r.assinatura.h)
    c.clip()
    c.font = `500 ${Math.round(28 * s)}px ${TIPO_MONO}`
    c.fillStyle = marca.cor
    c.textAlign = 'center'
    c.textBaseline = 'middle'
    c.fillText(
      marca.assinatura.trim(),
      r.assinatura.x + r.assinatura.w / 2,
      r.assinatura.y + r.assinatura.h / 2,
      r.assinatura.w,
    )
    c.textAlign = 'start'
    c.restore()
  }

  if (r.faixa) desenharFaixa(c, r.faixa, marca.faixa, marca.cor, instanteMs, largura)
}

/**
 * A moldura inteira, fundo e fontes incluídos — o atalho para quem não tem
 * maneira própria de desenhar as suas fontes. Chamar de dentro do
 * `requestAnimationFrame` do compositor, com o instante desse frame: a
 * animação da faixa sai daí, não de um relógio lido cá dentro.
 *
 * Devolve os rects que usou, para quem precise de saber onde as coisas foram
 * parar (um clique, um realce, um indicador de quem ficou de fora).
 */
export function desenharMoldura(
  c: CanvasRenderingContext2D,
  fontes: FontesDaMoldura,
  marca: MarcaDaMoldura,
  instanteMs: number,
  largura: number,
  altura: number,
  opcoes: Partial<OpcoesDaMoldura> = {},
): RectsDaMoldura {
  const r = rectsDaMoldura(fontes.camaras.length, largura, altura, opcoes)
  c.fillStyle = COR_DO_FUNDO
  c.fillRect(0, 0, largura, altura)
  pintar(c, fontes.heroi, r.heroi)
  r.camaras.forEach((rect, i) => pintar(c, fontes.camaras[i] ?? null, rect))
  desenharMarcaDaMoldura(c, r, marca, instanteMs, largura, altura)
  return r
}
