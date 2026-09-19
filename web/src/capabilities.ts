/**
 * Capacidades que o produto MOSTRA vs. capacidades que o produto FAZ.
 *
 * Este ficheiro existe por causa de um caso concreto (R109). Quem partilhava o
 * ecrã via um botão «pedir controlo», carregava «Aceitar» — e o outro lado
 * recebia a mensagem **«Pedido aceite — controlo remoto da tela partilhada
 * ativo»**. Não estava. Não há uma linha de código que encaminhe um clique ou
 * uma tecla para a máquina do outro; o handshake acaba na mensagem.
 *
 * A parte que interessa não é o botão que não funciona — é o CONSENTIMENTO que
 * não quer dizer nada. A pessoa foi informada de que estava a entregar o
 * controlo da sua máquina, disse que sim, e passou a comportar-se como se o
 * outro pudesse agir. Isso é pior do que não ter a funcionalidade.
 *
 * ── Porque é que não está construído, e não é preguiça ────────────────────
 *
 * Um browser **não consegue** injectar rato ou teclado no sistema operativo de
 * outra máquina. Nenhum. Não é uma API que falte à nossa implementação: é a
 * fronteira da sandbox, e é ela que faz do browser um sítio seguro para abrir
 * uma reunião. O Zoom e o Teams fazem-no porque instalam uma aplicação NATIVA
 * com permissões de acessibilidade/injecção de input no sistema.
 *
 * Ou seja: controlo remoto a sério = um agente nativo, com a superfície de
 * segurança que isso traz (uma porta para o teclado da vítima é exactamente o
 * que um atacante quer). Isso é um projecto com ADR próprio, não uma tarefa.
 *
 * ── O que fica ────────────────────────────────────────────────────────────
 *
 * O handshake de sinalização (`request` / `accept` / `deny`) FICA: é a base
 * correcta e já está testada. O que sai é a promessa. Quando o agente existir,
 * este ficheiro é o único sítio a mudar.
 */

/**
 * Existe um agente nativo capaz de receber rato e teclado nesta instalação?
 *
 * Enquanto for `false`: o botão de pedir controlo não é mostrado, e um pedido
 * que chegue de um cliente antigo é **recusado automaticamente** — em vez de
 * abrir um diálogo que pede um consentimento sem efeito.
 */
export const AGENTE_CONTROLO_REMOTO = false
