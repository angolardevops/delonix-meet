// Correcção de imagem por câmara em WebGL2 — exposição, temperatura, matiz,
// contraste e saturação num só passo. A matemática (e a referência que os
// testes medem) está em `studio/tv/correccao.ts`; aqui só se desenha.
//
// Sem WebGL2 cai para o `filter` do canvas 2D, que faz brilho, contraste e
// saturação mas NÃO temperatura nem matiz — `temTemperatura` diz à interface
// qual dos dois caminhos está activo, para não prometer o que não faz.

import { type CorreccaoDeImagem, multiplicadores } from '../studio/tv/correccao'
import { fullscreenTriangle, makeProgram, makeTexture } from './glUtil'

const FS = `#version 300 es
precision mediump float;
uniform sampler2D uTex;
uniform vec3 uMul;
uniform float uContraste;
uniform float uSat;
in vec2 vUv;
out vec4 outColor;
void main() {
  vec3 c = texture(uTex, vUv).rgb * uMul;
  c = (c - 0.5) * uContraste + 0.5;
  float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
  c = mix(vec3(l), c, uSat);
  outColor = vec4(clamp(c, 0.0, 1.0), 1.0);
}`

type Imagem = HTMLVideoElement | HTMLCanvasElement

const dims = (f: Imagem) =>
  f instanceof HTMLVideoElement ? { w: f.videoWidth, h: f.videoHeight } : { w: f.width, h: f.height }

/** Tecto do canvas corrigido: 1080p chega para o programa e poupa a GPU. */
const MAX_W = 1920
const MAX_H = 1080

export class CorrectorDeImagem {
  canvas = document.createElement('canvas')
  private gl: WebGL2RenderingContext | null = null
  private ctx2d: CanvasRenderingContext2D | null = null
  private prog: WebGLProgram | null = null
  private vao: WebGLVertexArrayObject | null = null
  private tex: WebGLTexture | null = null
  private u: Record<string, WebGLUniformLocation | null> = {}
  private perdido = false

  constructor() {
    const gl = this.canvas.getContext('webgl2', {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      // O canvas é lido pelo compositor noutro momento do frame.
      preserveDrawingBuffer: true,
    })
    if (gl) {
      try {
        this.prog = makeProgram(gl, FS)
        this.vao = fullscreenTriangle(gl)
        this.tex = makeTexture(gl)
        this.u = Object.fromEntries(['uTex', 'uMul', 'uContraste', 'uSat'].map((n) => [n, gl.getUniformLocation(this.prog!, n)]))
        this.gl = gl
        this.canvas.addEventListener('webglcontextlost', (e) => {
          e.preventDefault()
          this.perdido = true
        })
      } catch (e) {
        console.warn('[correcção] WebGL2 sem o shader', e)
        this.gl = null
      }
    }
    if (!this.gl) {
      // Um canvas que já deu um contexto WebGL não dá um 2D: troca-se de canvas.
      this.canvas = document.createElement('canvas')
      this.ctx2d = this.canvas.getContext('2d')
    }
  }

  /** `true` com WebGL2: temperatura e matiz funcionam. */
  get temTemperatura(): boolean {
    return !!this.gl && !this.perdido
  }

  /** Desenha `fonte` corrigida no próprio canvas. Devolve `false` se não havia imagem. */
  desenhar(fonte: Imagem, c: CorreccaoDeImagem): boolean {
    const { w, h } = dims(fonte)
    if (!w || !h) return false
    const esc = Math.min(1, MAX_W / w, MAX_H / h)
    const W = Math.round(w * esc)
    const H = Math.round(h * esc)
    if (this.canvas.width !== W || this.canvas.height !== H) {
      this.canvas.width = W
      this.canvas.height = H
    }
    const gl = this.gl
    if (gl && !this.perdido && this.prog && this.vao && this.tex) {
      gl.viewport(0, 0, W, H)
      gl.useProgram(this.prog)
      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, this.tex)
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, fonte)
      const m = multiplicadores(c)
      gl.uniform1i(this.u.uTex, 0)
      gl.uniform3f(this.u.uMul, m[0], m[1], m[2])
      gl.uniform1f(this.u.uContraste, c.contraste)
      gl.uniform1f(this.u.uSat, c.saturacao)
      gl.bindVertexArray(this.vao)
      gl.drawArrays(gl.TRIANGLES, 0, 3)
      gl.bindVertexArray(null)
      return true
    }
    const ctx = this.ctx2d
    if (!ctx) return false
    // Caminho 2D: o `filter` não tem temperatura; faz o que sabe fazer.
    ctx.filter = `brightness(${Math.pow(2, c.exposicao)}) contrast(${c.contraste}) saturate(${c.saturacao})`
    ctx.drawImage(fonte, 0, 0, W, H)
    ctx.filter = 'none'
    return true
  }

  destruir(): void {
    this.gl?.getExtension('WEBGL_lose_context')?.loseContext()
    this.gl = null
    this.canvas.width = 0
    this.canvas.height = 0
  }
}
