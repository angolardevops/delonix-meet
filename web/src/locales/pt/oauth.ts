// Consentimento de um agente (ADR-0028 §5). O texto de cada escopo é
// traduzido AQUI, e o do servidor (`McpScope::describes`) é o recuo para um
// escopo ainda por traduzir — o primeiro percurso ponta-a-ponta mostrou o ecrã
// em inglês com os escopos em português, e num ecrã de permissões isso não
// serve.
export default {
  consent: {
    escopos: {
      'meet.read': 'Ver as reuniões, salas e participantes a que já tens acesso',
      'minutes.read': 'Ler a acta e a transcrição dessas reuniões',
      'minutes.write': 'Gerar e gravar a acta de uma reunião',
      'recordings.read': 'Ver a tua biblioteca de gravações, com legendas e capítulos',
      'captions.write': 'Gerar e traduzir legendas de uma gravação',
      'chapters.write': 'Gerar e editar capítulos de uma gravação',
      'clips.write': 'Criar recortes e vídeos curtos a partir de uma gravação',
      'shorts.publish': 'Publicar um vídeo curto num destino da organização',
      'live.translate': 'Ligar e configurar a tradução ao vivo numa sala tua',
    },
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
