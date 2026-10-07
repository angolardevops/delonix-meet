/**
 * Os destinos de emissão da ORGANIZAÇÃO, do servidor
 * (`/api/orgs/{org}/stream-destinations`).
 *
 * Até 2026-10-06 o Estúdio guardava-os num `useState` que arrancava com um
 * YouTube escrito à mão e chave vazia: **a chave RTMP tinha de ser reescrita a
 * cada F5**, e o CRUD completo que o servidor já tinha — criar, editar, apagar,
 * rotar a chave — não era chamado por ecrã nenhum.
 *
 * A chave **não volta ao browser**, e isso é desenho do servidor: fica cifrada
 * em repouso (`stream_key_sealed`) e um destino guardado vai na emissão só pelo
 * `id` (`pedidoDeInicio` em `directo.ts`). Por isso um destino carregado tem
 * `chave: ''` e `temChaveGuardada: true` — são coisas diferentes, e a interface
 * tem de as distinguir para não oferecer «ir para o ar» a um destino sem chave
 * nem dizer «sem chave» a um que tem.
 *
 * Este módulo é puro: o que fala com a rede é o `Studio`.
 */
import type { StreamDestination, StreamKind } from '../api'
import type { Destino } from './directo'

/**
 * O `kind` que o servidor exige, derivado do URL. É uma etiqueta para a
 * interface e para as métricas — o que manda na emissão é o URL. Um URL que não
 * se reconhece é `rtmp`, que é o caso geral.
 */
export function kindDoUrl(url: string): StreamKind {
  const u = url.trim().toLowerCase()
  if (u.includes('youtube.com') || u.includes('youtu.be')) return 'youtube'
  if (u.includes('facebook.com') || u.includes('fb.me')) return 'facebook'
  if (u.includes('linkedin.com')) return 'linkedin'
  return 'rtmp'
}

/** Um destino guardado, como o Estúdio o usa. */
export function deDestinoGuardado(d: StreamDestination): Destino {
  return {
    id: d.id,
    url: d.url,
    // A chave vive no servidor: o que o browser sabe é que ela existe.
    chave: '',
    temChaveGuardada: d.has_key,
    rotulo: d.label,
  }
}

/**
 * O que fazer no servidor quando um destino é editado no diálogo.
 *
 * Decidido aqui, e não no meio do handler, porque são quatro casos e três deles
 * eram fáceis de esquecer:
 * - **nada**: ainda não há URL — um cartão aberto e fechado sem se escrever não
 *   cria lixo na organização;
 * - **criar**: sem `id` e com URL;
 * - **actualizar**: com `id`, se o rótulo ou o URL mudaram;
 * - **rotar a chave**: com `id` e uma chave escrita de novo. É uma operação à
 *   parte porque substituir uma credencial não é editar um nome, e o servidor
 *   responde-lhe com a chave nova uma única vez.
 *
 * `actualizar` e `rotarChave` podem vir os dois no mesmo acto: quem corrige o
 * URL e cola a chave nova de uma vez faz as duas coisas.
 */
export type AccaoNoServidor = {
  criar?: { kind: StreamKind; label: string; url: string; stream_key?: string }
  actualizar?: { label?: string; url?: string }
  rotarChave?: string
}

export function accaoDeGuardar(antes: Destino | undefined, agora: Destino): AccaoNoServidor {
  const url = agora.url.trim()
  const chave = agora.chave.trim()
  const rotulo = (agora.rotulo ?? '').trim()
  if (!url) return {}
  if (!agora.id) {
    return {
      criar: {
        kind: kindDoUrl(url),
        label: rotulo || kindDoUrl(url),
        url,
        ...(chave ? { stream_key: chave } : {}),
      },
    }
  }
  const accao: AccaoNoServidor = {}
  const mudou: { label?: string; url?: string } = {}
  if (antes && url !== antes.url.trim()) mudou.url = url
  if (antes && rotulo !== (antes.rotulo ?? '').trim()) mudou.label = rotulo
  if (Object.keys(mudou).length > 0) accao.actualizar = mudou
  if (chave) accao.rotarChave = chave
  return accao
}

/**
 * Um destino tem chave se foi escrita agora OU se o servidor a guarda. Sem
 * isto, um destino carregado do servidor lia-se como «sem chave» e o botão «ir
 * para o ar» ficava desligado com tudo configurado.
 */
export const temChave = (d: Destino): boolean => !!d.chave.trim() || !!d.temChaveGuardada
