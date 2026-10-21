/**
 * O backoffice (`web-admin/`) só funciona servido do MESMO origin que a API —
 * sem `allow_credentials` no CORS, a sessão por cookie não atravessa origins —
 * e por isso vive em `/admin/` dentro da imagem `web`. São quatro ficheiros
 * que têm de concordar, em três linguagens, e nenhum build falha se um deles
 * mudar sozinho: o `base` do Vite, as cópias nos dois Dockerfiles e as
 * `location` do nginx. Um `base` esquecido dá uma página em branco (assets
 * pedidos em `/assets/…`, servidos pela app do cliente), sem erro no deploy.
 *
 * Vive na suite do `web/` e não na do `web-admin/` porque o Vite do
 * `web-admin` recusa ler fora da sua raiz, e alargar o `server.fs.allow`
 * abria o repositório ao servidor de desenvolvimento (que escuta em 0.0.0.0).
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const root = join(__dirname, '..', '..')
const read = (p: string) => readFileSync(join(root, p), 'utf8')

describe('o backoffice em /admin/, no mesmo origin da API', () => {
  it('o Vite do web-admin gera os assets debaixo de /admin/', () => {
    expect(read('web-admin/vite.config.ts')).toMatch(/^\s*base:\s*'\/admin\/'/m)
  })

  it('as duas imagens web levam o dist do backoffice para /admin', () => {
    expect(read('Dockerfile.web')).toMatch(
      /COPY --from=build-admin \/web-admin\/dist \/usr\/share\/nginx\/html\/admin/,
    )
    expect(read('Dockerfile.web.stage')).toMatch(/COPY web-admin\/dist \/usr\/share\/nginx\/html\/admin/)
  })

  const nginx = read('deploy/k8s/nginx.conf')

  it('um caminho desconhecido cai no index DO BACKOFFICE, não no da app do cliente', () => {
    expect(nginx).toMatch(/location \^~ \/admin\/ \{[^}]*try_files \$uri \/admin\/index\.html;/)
  })

  it('o redirect de /admin é relativo (atrás do Ingress, um absoluto levava :8080)', () => {
    expect(nginx).toMatch(/location = \/admin \{[^}]*absolute_redirect off;[^}]*return 301 \/admin\/;/)
  })
})
