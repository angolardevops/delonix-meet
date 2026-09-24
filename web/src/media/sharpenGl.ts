// Unsharp mask em WebGL2, num só passo. A matemática e os limites estão em
// `sharpen.ts`; aqui só se desenha.

import { fullscreenTriangle, makeProgram, makeTexture, overlayContext } from './glUtil'
import { kernel3x3, unsharpParams } from './sharpen'
import type { OverlayRenderer } from './videoOverlay'

const FS = `#version 300 es
precision mediump float;
uniform sampler2D uTex;
uniform vec2 uStep;       // raio em UV de saída
uniform vec3 uK;          // centro, cruz, diagonal
uniform float uAmount;
uniform float uThreshold;
in vec2 vUv;
out vec4 outColor;
float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }
void main() {
  vec3 c = texture(uTex, vUv).rgb;
  vec2 dx = vec2(uStep.x, 0.0);
  vec2 dy = vec2(0.0, uStep.y);
  vec3 cross = texture(uTex, vUv + dx).rgb + texture(uTex, vUv - dx).rgb
             + texture(uTex, vUv + dy).rgb + texture(uTex, vUv - dy).rgb;
  vec3 diag = texture(uTex, vUv + dx + dy).rgb + texture(uTex, vUv - dx - dy).rgb
            + texture(uTex, vUv + dx - dy).rgb + texture(uTex, vUv - dx + dy).rgb;
  vec3 blur = c * uK.x + cross * uK.y + diag * uK.z;
  // Só na luminância: realçar cada canal à parte cria franjas de cor.
  float d = luma(c) - luma(blur);
  // Limiar suave — diferenças pequenas (ruído, blocos) passam sem realce.
  float gate = smoothstep(uThreshold, uThreshold * 3.0, abs(d));
  outColor = vec4(clamp(c + d * uAmount * gate, 0.0, 1.0), 1.0);
}`

export class UnsharpRenderer implements OverlayRenderer {
  /** 0–1, escolhido na interface. */
  strength = 0.5
  lost = false
  private gl: WebGL2RenderingContext
  private prog: WebGLProgram
  private vao: WebGLVertexArrayObject
  private tex: WebGLTexture
  private u: Record<string, WebGLUniformLocation | null>

  private constructor(private canvas: HTMLCanvasElement, gl: WebGL2RenderingContext) {
    this.gl = gl
    this.prog = makeProgram(gl, FS)
    this.vao = fullscreenTriangle(gl)
    this.tex = makeTexture(gl)
    this.u = Object.fromEntries(['uTex', 'uStep', 'uK', 'uAmount', 'uThreshold'].map((n) => [n, gl.getUniformLocation(this.prog, n)]))
    canvas.addEventListener('webglcontextlost', this.onLost)
  }

  /** `null` sem WebGL2 — e aí o realce fica desligado, não há imitação em 2D. */
  static create(canvas: HTMLCanvasElement): UnsharpRenderer | null {
    const gl = overlayContext(canvas)
    if (!gl) return null
    try {
      return new UnsharpRenderer(canvas, gl)
    } catch (e) {
      console.warn('[nitidez] WebGL2 sem o shader', e)
      return null
    }
  }

  private onLost = (e: Event) => {
    e.preventDefault()
    this.lost = true
  }

  render(video: HTMLVideoElement, width: number, height: number): void {
    if (this.lost) return
    const gl = this.gl
    gl.viewport(0, 0, width, height)
    gl.useProgram(this.prog)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, this.tex)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video)
    const p = unsharpParams(this.strength, video.videoHeight, height)
    const k = kernel3x3(p.radius)
    gl.uniform1i(this.u.uTex, 0)
    gl.uniform2f(this.u.uStep, p.radius / width, p.radius / height)
    gl.uniform3f(this.u.uK, k.center, k.edge, k.corner)
    gl.uniform1f(this.u.uAmount, p.amount)
    gl.uniform1f(this.u.uThreshold, p.threshold)
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
    gl.deleteTexture(this.tex)
    gl.deleteProgram(this.prog)
    gl.deleteVertexArray(this.vao)
    gl.getExtension('WEBGL_lose_context')?.loseContext()
  }
}
