// Agent consent (ADR-0028 §5). Each scope's text is translated HERE, and the
// server's (`McpScope::describes`) is the fallback for a scope not translated
// yet — the first end-to-end run showed the screen in English with the scopes
// in Portuguese, and on a permissions screen that will not do.
export default {
  consent: {
    escopos: {
      'meet.read': 'See the meetings, rooms and participants you already have access to',
      'minutes.read': 'Read the minutes and transcript of those meetings',
      'minutes.write': 'Generate and save a meeting’s minutes',
      'recordings.read': 'See your recording library, with captions and chapters',
      'captions.write': 'Generate and translate a recording’s captions',
      'chapters.write': 'Generate and edit a recording’s chapters',
      'clips.write': 'Create clips and short videos from a recording',
      'shorts.publish': 'Publish a short video to an organisation destination',
      'live.translate': 'Turn on and configure live translation in a room of yours',
    },
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
