// Palco imersivo em WebGL2: fundo e pessoa como duas camadas do MESMO frame,
// separadas pela máscara do segmentador.
//
//   1. Fundo — o vídeo ampliado (overscan), deslocado com a paralaxe e
//      desfocado por mipmap (profundidade de campo barata: um `textureLod`).
//      Onde a própria pessoa estava no fundo fica uma silhueta desfocada e
//      escurecida, que se lê como sombra na parede em vez de «fantasma».
//   2. Sombra — a máscara, muito desfocada, projectada no fundo.
//   3. Pessoa — o vídeo nítido recortado pela máscara, deslocado no sentido
//      oposto, com micro-movimento, luz principal suave e um contorno de luz.
//
// O contexto WebGL é PARTILHADO com o segmentador do MediaPipe (ver
// `personSegmenter.ts`): a máscara nunca sai da GPU. O preço é que o MediaPipe
// mexe no estado do contexto — por isso cada passo aqui repõe tudo aquilo de
// que depende, em vez de confiar no que ficou de antes.
//
// Tudo neste dispositivo, por cima do <video> do retrato. Nada é enviado.

import { FULLSCREEN_VS, makeProgram, overlayContext } from '../../media/glUtil'
import type { OverlayRenderer } from '../../media/videoOverlay'
import { MASK_RAMP, type LayerOffsets, type Vec } from './parallax'
import type { MaskSink } from './personSegmenter'

const FS = `#version 300 es
precision mediump float;
uniform sampler2D uVideo;
uniform sampler2D uMask;
uniform float uHasMask;
uniform vec2 uBg;
uniform vec2 uFg;
uniform vec2 uShadow;
uniform float uOver;
uniform float uFgScale;
uniform float uLift;
uniform float uDof;
uniform float uFade;
uniform vec2 uLight;
in vec2 vUv;
out vec4 outColor;
vec2 zoom(vec2 uv, float s, vec2 off) { return (uv - 0.5) / s + 0.5 - off; }
void main() {
  vec2 bgUv = zoom(vUv, uOver, uBg);
  vec3 bg = textureLod(uVideo, bgUv, uDof).rgb;
  float ghost = uHasMask * textureLod(uMask, bgUv, 2.0).r;
  bg *= 1.0 - 0.45 * ghost;
  float vig = smoothstep(0.95, 0.25, distance(vUv, vec2(0.5, 0.46)));
  bg *= mix(0.62, 1.0, vig);

  vec2 fgUv = zoom(vUv, uFgScale, uFg + vec2(0.0, uLift));
  float shadow = uHasMask * textureLod(uMask, fgUv - uShadow, 3.0).r;
  bg *= 1.0 - 0.35 * shadow;

  vec3 person = texture(uVideo, fgUv).rgb;
  // Antes da primeira máscara a pessoa é o frame inteiro: o vídeo aparece
  // nítido logo, e as camadas entram quando o recorte chega.
  float a = uHasMask > 0.5 ? texture(uMask, fgUv).r : 1.0;
  float key = 1.0 + 0.10 * smoothstep(0.9, 0.0, distance(vUv, uLight));
  person *= key;
  person += vec3(clamp(a * (1.0 - a) * 4.0, 0.0, 1.0) * 0.06);

  vec3 col = mix(bg, person, a);
  outColor = vec4(col * (1.0 - uFade), 1.0);
}`

// Confiança → alfa com a rampa do BackgroundEffect, misturada com a máscara
// anterior: sem isso a borda cintila, e a paralaxe amplia cada hesitação.
const MASK_FS = `#version 300 es
precision mediump float;
uniform sampler2D uNew;
uniform sampler2D uPrev;
uniform float uKeep;
in vec2 vUv;
out vec4 outColor;
void main() {
  float c = texture(uNew, vUv).r;
  float a = smoothstep(${MASK_RAMP.lo.toFixed(2)}, ${MASK_RAMP.hi.toFixed(2)}, c);
  float p = texture(uPrev, vUv).r;
  outColor = vec4(mix(a, p, uKeep), 0.0, 0.0, 1.0);
}`

/** A máscara vive a esta resolução: é desfocada por desenho, e 1280×720 floats não compram nada. */
const MASK_W = 320
const MASK_H = 180

export interface ImmersiveFrame {
  offsets: LayerOffsets
  /** Escala da pessoa (micro-movimento × transição). */
  fgScale: number
  lift: number
  fade: number
  light: Vec
  /** Nível de mipmap do fundo — a profundidade de campo. */
  dof: number
}

export class ImmersiveRenderer implements OverlayRenderer {
  lost = false
  frame: ImmersiveFrame = {
    offsets: { bg: { x: 0, y: 0 }, fg: { x: 0, y: 0 }, shadow: { x: 0.012, y: 0.02 }, overscan: 1 },
    fgScale: 1,
    lift: 0,
    fade: 0,
    light: { x: 0.35, y: 0.25 },
    dof: 2.5,
  }
  /** Quantas máscaras chegaram à textura (prova de que há recorte, não só vídeo). */
  masksUploaded = 0
  readonly gl: WebGL2RenderingContext
  private prog: WebGLProgram
  private maskProg: WebGLProgram
  private vao: WebGLVertexArrayObject
  private buf: WebGLBuffer
  private videoTex: WebGLTexture
  /** Ping-pong: a máscara actual e a anterior. */
  private masks: [WebGLTexture, WebGLTexture]
  private fbos: [WebGLFramebuffer, WebGLFramebuffer]
  private current = 0
  /** Upload do caminho CPU (sem textura do MediaPipe). */
  private cpuTex: WebGLTexture | null = null
  private u: Record<string, WebGLUniformLocation | null>
  private mu: Record<string, WebGLUniformLocation | null>
  private hasMask = false
  private floatLinear: boolean

  private constructor(readonly canvas: HTMLCanvasElement, gl: WebGL2RenderingContext) {
    this.gl = gl
    this.prog = makeProgram(gl, FS, FULLSCREEN_VS)
    this.maskProg = makeProgram(gl, MASK_FS, FULLSCREEN_VS)
    // VAO e buffer próprios: o MediaPipe usa os seus e desliga atributos.
    this.vao = gl.createVertexArray()!
    this.buf = gl.createBuffer()!
    gl.bindVertexArray(this.vao)
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buf)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW)
    gl.enableVertexAttribArray(0)
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
    gl.bindVertexArray(null)
    this.floatLinear = !!gl.getExtension('OES_texture_float_linear')
    this.videoTex = this.tex(true)
    const mk = () => {
      const t = this.tex(true)
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, MASK_W, MASK_H, 0, gl.RED, gl.UNSIGNED_BYTE, null)
      const fb = gl.createFramebuffer()!
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb)
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, t, 0)
      gl.bindFramebuffer(gl.FRAMEBUFFER, null)
      return [t, fb] as const
    }
    const a = mk()
    const b = mk()
    this.masks = [a[0], b[0]]
    this.fbos = [a[1], b[1]]
    const names = ['uVideo', 'uMask', 'uHasMask', 'uBg', 'uFg', 'uShadow', 'uOver', 'uFgScale', 'uLift', 'uDof', 'uFade', 'uLight']
    this.u = Object.fromEntries(names.map((n) => [n, gl.getUniformLocation(this.prog, n)]))
    this.mu = Object.fromEntries(['uNew', 'uPrev', 'uKeep'].map((n) => [n, gl.getUniformLocation(this.maskProg, n)]))
    canvas.addEventListener('webglcontextlost', this.onLost)
  }

  static create(canvas: HTMLCanvasElement): ImmersiveRenderer | null {
    const gl = overlayContext(canvas)
    if (!gl) return null
    try {
      return new ImmersiveRenderer(canvas, gl)
    } catch (e) {
      console.warn('[imersivo] WebGL2 sem o shader', e)
      return null
    }
  }

  private tex(mipmaps: boolean): WebGLTexture {
    const gl = this.gl
    const t = gl.createTexture()!
    gl.bindTexture(gl.TEXTURE_2D, t)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, mipmaps ? gl.LINEAR_MIPMAP_LINEAR : gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    return t
  }

  private onLost = (e: Event) => {
    e.preventDefault()
    this.lost = true
  }

  /** O estado de que estes passos dependem — o MediaPipe mexe no mesmo contexto. */
  private resetState() {
    const gl = this.gl
    gl.disable(gl.BLEND)
    gl.disable(gl.DEPTH_TEST)
    gl.disable(gl.STENCIL_TEST)
    gl.disable(gl.SCISSOR_TEST)
    gl.disable(gl.CULL_FACE)
    gl.colorMask(true, true, true, true)
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false)
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false)
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 4)
    gl.bindVertexArray(this.vao)
  }

  /** Máscara nova. Chamado DENTRO da chamada do segmentador (a textura dele expira). */
  acceptMask(m: MaskSink): void {
    if (this.lost) return
    const gl = this.gl
    this.resetState()
    let src: WebGLTexture
    if (m.kind === 'texture') {
      src = m.texture
      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, src)
      // A textura é float: filtro linear só com a extensão, senão amostragem directa.
      const f = this.floatLinear ? gl.LINEAR : gl.NEAREST
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, f)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, f)
    } else {
      if (!this.cpuTex) {
        this.cpuTex = gl.createTexture()!
        gl.bindTexture(gl.TEXTURE_2D, this.cpuTex)
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
      }
      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, this.cpuTex)
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1)
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R32F, m.width, m.height, 0, gl.RED, gl.FLOAT, m.data)
      src = this.cpuTex
    }
    const next = 1 - this.current
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.fbos[next])
    gl.viewport(0, 0, MASK_W, MASK_H)
    gl.useProgram(this.maskProg)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.masks[this.current])
    gl.uniform1i(this.mu.uNew, 0)
    gl.uniform1i(this.mu.uPrev, 1)
    gl.uniform1f(this.mu.uKeep, this.hasMask ? 0.35 : 0)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    gl.bindTexture(gl.TEXTURE_2D, this.masks[next])
    gl.generateMipmap(gl.TEXTURE_2D)
    this.current = next
    this.hasMask = true
    this.masksUploaded++
  }

  render(video: HTMLVideoElement, width: number, height: number): void {
    if (this.lost) return
    const gl = this.gl
    this.resetState()
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    gl.viewport(0, 0, width, height)
    gl.useProgram(this.prog)

    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, this.videoTex)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video)
    gl.generateMipmap(gl.TEXTURE_2D)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.masks[this.current])

    const f = this.frame
    gl.uniform1i(this.u.uVideo, 0)
    gl.uniform1i(this.u.uMask, 1)
    gl.uniform1f(this.u.uHasMask, this.hasMask ? 1 : 0)
    gl.uniform2f(this.u.uBg, f.offsets.bg.x, f.offsets.bg.y)
    gl.uniform2f(this.u.uFg, f.offsets.fg.x, f.offsets.fg.y)
    gl.uniform2f(this.u.uShadow, f.offsets.shadow.x, f.offsets.shadow.y)
    gl.uniform1f(this.u.uOver, f.offsets.overscan)
    gl.uniform1f(this.u.uFgScale, f.fgScale)
    gl.uniform1f(this.u.uLift, f.lift)
    gl.uniform1f(this.u.uDof, f.dof)
    gl.uniform1f(this.u.uFade, f.fade)
    gl.uniform2f(this.u.uLight, f.light.x, f.light.y)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
  }

  finish(): void {
    if (!this.lost) this.gl.finish()
  }

  /** Apaga os recursos e perde o contexto. Só quando o efeito deixa de ser pedido. */
  destroy(): void {
    this.canvas.removeEventListener('webglcontextlost', this.onLost)
    if (this.lost) return
    const gl = this.gl
    for (const t of [this.videoTex, ...this.masks, this.cpuTex]) if (t) gl.deleteTexture(t)
    for (const fb of this.fbos) gl.deleteFramebuffer(fb)
    gl.deleteProgram(this.prog)
    gl.deleteProgram(this.maskProg)
    gl.deleteBuffer(this.buf)
    gl.deleteVertexArray(this.vao)
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    this.lost = true
  }
}
