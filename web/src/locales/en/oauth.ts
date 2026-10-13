// Agent consent (ADR-0028 §5). Each scope's text comes from the server
// (`McpScope::describes`): adding a scope must never leave an untranslated
// sentence on a screen where permissions are granted.
export default {
  consent: {
    titulo: 'Authorise an agent',
    pedido: '{{cliente}} is requesting access to your account in the {{organizacao}} organisation.',
    registoProprio: 'This client registered itself and returns access to {{destino}}. Nobody approved it before you — make sure it is the program you are actually using.',
    vaiPoderMexer: 'It will be able to change',
    vaiPoderVer: 'It will be able to see',
    agePorTi: 'The agent acts as you: it sees what you see, and nothing more. Whatever it does is recorded in the audit trail under your name.',
    jaConsentido: 'You had already authorised this agent with these permissions.',
    autorizar: 'Authorise',
    negar: 'Do not authorise',
    aCarregar: 'Reading the request…',
    erroPedido: 'This address does not carry a complete authorisation request. Start again from the program that opened it.',
    revogarDepois: 'You can withdraw this authorisation whenever you like, in My account. Withdrawing it ends the agent session immediately.',
    voltar: 'Back to the start',
  },
}
