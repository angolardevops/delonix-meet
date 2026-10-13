// Consentement d'un agent (ADR-0028 §5). Le texte de CHAQUE portée vient du
// serveur (`McpScope::describes`) : ajouter une portée ne doit jamais laisser
// une phrase non traduite sur un écran où l'on accorde des permissions.
export default {
  consent: {
    titulo: 'Autoriser un agent',
    pedido: '{{cliente}} demande l’accès à votre compte dans l’organisation {{organizacao}}.',
    registoProprio: 'Ce client s’est enregistré lui-même et renvoie l’accès vers {{destino}}. Personne ne l’a approuvé avant vous — vérifiez qu’il s’agit bien du programme que vous utilisez.',
    vaiPoderMexer: 'Il pourra modifier',
    vaiPoderVer: 'Il pourra voir',
    agePorTi: 'L’agent agit en votre nom : il voit ce que vous voyez, et rien de plus. Ce qu’il fait est inscrit dans la piste d’audit à votre nom.',
    jaConsentido: 'Vous aviez déjà autorisé cet agent avec ces permissions.',
    autorizar: 'Autoriser',
    negar: 'Ne pas autoriser',
    aCarregar: 'Lecture de la demande…',
    erroPedido: 'Cette adresse ne contient pas une demande d’autorisation complète. Recommencez depuis le programme qui l’a ouverte.',
    revogarDepois: 'Vous pouvez retirer cette autorisation quand vous voulez, dans Mon compte. La retirer met fin à la session de l’agent immédiatement.',
    voltar: 'Retour à l’accueil',
  },
}
