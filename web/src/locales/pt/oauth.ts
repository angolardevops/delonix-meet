// Consentimento de um agente (ADR-0028 §5). O texto de CADA escopo vem do
// servidor (`McpScope::describes`): acrescentar um escopo não pode deixar uma
// frase por traduzir num ecrã onde se dão permissões.
export default {
  consent: {
    titulo: 'Autorizar um agente',
    pedido: '{{cliente}} pede acesso à tua conta na organização {{organizacao}}.',
    registoProprio: 'Este cliente registou-se a si mesmo e devolve o acesso a {{destino}}. Ninguém o aprovou antes de ti — confirma que é mesmo o programa que estás a usar.',
    vaiPoderMexer: 'Vai poder mexer',
    vaiPoderVer: 'Vai poder ver',
    agePorTi: 'O agente age como tu: vê o que tu vês, e nada mais. O que ele fizer fica na trilha de auditoria com o teu nome.',
    jaConsentido: 'Já tinhas autorizado este agente com estas permissões.',
    autorizar: 'Autorizar',
    negar: 'Não autorizar',
    aCarregar: 'A ler o pedido…',
    erroPedido: 'Este endereço não traz um pedido de autorização completo. Volta a começar no programa que o abriu.',
    revogarDepois: 'Podes retirar esta autorização quando quiseres, em A minha conta. Retirá-la termina a sessão do agente de imediato.',
    voltar: 'Voltar ao início',
  },
}
