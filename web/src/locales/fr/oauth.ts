// Consentement d'un agent (ADR-0028 §5). Le texte de chaque portée est traduit
// ICI ; celui du serveur (`McpScope::describes`) sert de repli pour une portée
// pas encore traduite — le premier parcours de bout en bout a montré l'écran en
// anglais avec les portées en portugais, ce qui ne convient pas à un écran de
// permissions.
export default {
  consent: {
    escopos: {
      'meet.read': 'Voir les réunions, salles et participants auxquels vous avez déjà accès',
      'minutes.read': 'Lire le compte rendu et la transcription de ces réunions',
      'minutes.write': 'Générer et enregistrer le compte rendu d’une réunion',
      'recordings.read': 'Voir votre bibliothèque d’enregistrements, avec sous-titres et chapitres',
      'captions.write': 'Générer et traduire les sous-titres d’un enregistrement',
      'chapters.write': 'Générer et modifier les chapitres d’un enregistrement',
      'clips.write': 'Créer des extraits et des vidéos courtes à partir d’un enregistrement',
      'shorts.publish': 'Publier une vidéo courte vers une destination de l’organisation',
      'live.translate': 'Activer et configurer la traduction en direct dans une de vos salles',
    },
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
