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
// Tudo neste dispositivo, por cima do <video> do retrato. Nada é enviado.

import { fullscreenTriangle, makeProgram, makeTexture, overlayContext } from '../../media/glUtil'
import type { OverlayRenderer } from '../../media/videoOverlay'
import type { LayerOffsets, Vec } from './parallax'

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
  /** Quantas máscaras chegaram à GPU (prova de que há recorte, não só vídeo). */
  masksUploaded = 0
  private gl: WebGL2RenderingContext
  private prog: WebGLProgram
  private vao: WebGLVertexArrayObject
  private videoTex: WebGLTexture
  private maskTex: WebGLTexture
  private u: Record<string, WebGLUniformLocation | null>
  private pendingMask: { data: Uint8Array; w: number; h: number } | null = null
  private hasMask = false

  private constructor(private canvas: HTMLCanvasElement, gl: WebGL2RenderingContext) {
    this.gl = gl
    this.prog = makeProgram(gl, FS)
    this.vao = fullscreenTriangle(gl)
    this.videoTex = makeTexture(gl, true)
    this.maskTex = makeTexture(gl, true)
    const names = ['uVideo', 'uMask', 'uHasMask', 'uBg', 'uFg', 'uShadow', 'uOver', 'uFgScale', 'uLift', 'uDof', 'uFade', 'uLight']
    this.u = Object.fromEntries(names.map((n) => [n, gl.getUniformLocation(this.prog, n)]))
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

  private onLost = (e: Event) => {
    e.preventDefault()
    this.lost = true
  }

  /** Máscara nova (0–255, uma por píxel). Sobe para a GPU no próximo frame. */
  setMask(data: Uint8Array, w: number, h: number): void {
    this.pendingMask = { data, w, h }
  }

  render(video: HTMLVideoElement, width: number, height: number): void {
    if (this.lost) return
    const gl = this.gl
    gl.viewport(0, 0, width, height)
    gl.useProgram(this.prog)

    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, this.videoTex)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video)
    gl.generateMipmap(gl.TEXTURE_2D)

    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.maskTex)
    if (this.pendingMask) {
      const m = this.pendingMask
      this.pendingMask = null
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1)
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, m.w, m.h, 0, gl.RED, gl.UNSIGNED_BYTE, m.data)
      gl.generateMipmap(gl.TEXTURE_2D)
      this.hasMask = true
      this.masksUploaded++
    }

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
    gl.bindVertexArray(this.vao)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
  }

  finish(): void {
    if (!this.lost) this.gl.finish()
  }

  destroy(): void {
    this.canvas.removeEventListener('webglcontextlost', this.onLost)
    if (this.lost) return
    const gl = this.gl
    gl.deleteTexture(this.videoTex)
    gl.deleteTexture(this.maskTex)
    gl.deleteProgram(this.prog)
    gl.deleteVertexArray(this.vao)
    gl.getExtension('WEBGL_lose_context')?.loseContext()
  }
}
