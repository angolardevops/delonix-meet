// English dictionary for the operator backoffice — same shape as `pt/index.ts`.

const ui = {
  erroCarregar: 'Could not load.',
  tentarDeNovo: 'Try again',
  cancelar: 'Cancel',
  confirmar: 'Confirm',
  fechar: 'Close',
  guardar: 'Save',
  carregarMais: 'Load more',
  rotaDesconhecida: {
    titulo: 'Page not found',
    texto: 'This address does not match any backoffice section.',
    inicio: 'Go to Overview',
  },
}

const shell = {
  nav: {
    overview: 'Overview',
    tenants: 'Organizations',
    integrations: 'Storage',
    security: 'Security',
    communications: 'Communications',
  },
  marca: 'Administration',
  terminarSessao: 'Sign out',
  saltarParaConteudo: 'Skip to content',
  abrirNavegacao: 'Open navigation',
}

const auth = {
  titulo: 'Sign in',
  subtitulo: 'Delonix Meet platform administration',
  email: 'Email',
  password: 'Password',
  entrar: 'Sign in',
  erro: 'Could not sign in.',
}

const nodes = {
  titulo: 'Overview',
  semAcesso: {
    titulo: 'No operator access',
    texto: 'Your account is not on the platform administrators list. Ask an existing operator to add you to PLATFORM_ADMIN_USER_IDS.',
  },
  semSuperficie: {
    titulo: 'No operator surface',
    texto: 'This installation does not expose an operator surface.',
  },
  resumo: {
    serving: 'Serving',
    draining: 'Draining',
    unreachable: 'Unreachable',
    peers: 'Participants',
  },
  tabela: {
    hostname: 'Node',
    versao: 'Version',
    estado: 'Status',
    carga: 'Load',
    salas: 'Rooms',
    participantes: 'Participants',
    visto: 'Last seen',
  },
  estado: {
    serving: 'serving',
    draining: 'draining',
    unreachable: 'unreachable',
  },
  vazio: 'No media nodes registered.',
}

const tenants = {
  titulo: 'Organizations',
  tabela: {
    nome: 'Name',
    dominio: 'Domain',
    membros: 'Members',
    grupos: 'Groups',
    salas: 'Rooms',
    reunioes: 'Meetings',
    armazenamento: 'Storage',
    lugares: 'Seats',
    participantes: 'Participants',
  },
  ilimitado: 'unlimited',
  vazio: 'No organizations found.',
  detalhe: {
    titulo: 'Organization detail',
    fechar: 'Close detail',
    uso: 'Current usage',
    lugaresUsados: '{{used}} used · {{active}} active this month',
    armazenamentoUsado: 'used',
  },
  quotas: {
    titulo: 'Plan quotas',
    grupos: 'Max groups',
    salas: 'Max rooms',
    reunioes: 'Max meetings',
    vozBackend: 'Voice backend',
    vozDid: 'Number model',
    manterActual: '(keep current)',
    backend: { freeswitch: 'FreeSWITCH', provider: 'Carrier' },
    did: { shared: 'Shared', dedicated: 'Dedicated' },
  },
  seats: {
    titulo: 'Seats',
    max: 'Seat cap',
  },
  concurrency: {
    titulo: 'Concurrency',
    max: 'Concurrent participants per node',
  },
  guardado: 'Saved.',
  erroGuardar: 'Could not save.',
}

const storage = {
  titulo: 'Recordings storage',
  sub: 'Platform storage destination — not the organization\'s.',
  soPlataforma: 'Only the platform can see and change this.',
  destino: 'Destination',
  tipo: { local: 'Local', nfs: 'NFS', webdav: 'WebDAV' },
  tipoDica: {
    local: 'Node disk — no redundancy across nodes.',
    nfs: 'NFS share mounted by the media nodes.',
    webdav: 'External WebDAV server.',
  },
  emUso: 'in use',
  nfsDica: 'The node mounts this share before writing recordings.',
  nfsServidor: 'NFS server',
  nfsCaminho: 'Path',
  manifesto: 'Download PVC manifest',
  manifestoGuardarPrimeiro: 'Save first to download the manifest.',
  manifestoFalhou: 'Could not get the manifest.',
  webdavDica: 'Credentials are stored encrypted; the password is never shown again.',
  webdavUrl: 'URL',
  webdavUtilizador: 'User',
  webdavPassword: 'Password',
  passwordDefinida: 'A password is already set.',
  webdavCaminho: 'Path',
  objectosTitulo: 'Object storage',
  objectosSub: 'Configured on the server, but not yet used by recordings.',
  objectosEndpoint: 'Endpoint',
  objectosBucket: 'Bucket',
  objectosAindaNao: 'Recordings still live on disk (ADR-0020) — this bucket does not receive them yet.',
  testar: 'Test',
  testeOk: 'Connection succeeded.',
  testeFalhou: 'The connection failed.',
  testaGravado: 'Save to test the new destination.',
  guardado: 'Saved.',
  erroGuardar: 'Could not save.',
}

const loginSettings = {
  titulo: 'Platform sign-in',
  hideOrgCreation: 'Hide organization creation',
  hideOrgCreationHint: 'The sign-in screen stops offering "create a new organization".',
  hideSsoButton: 'Hide SSO button',
  hideSsoButtonHint: 'The sign-in screen stops showing the per-domain SSO shortcut.',
  guardado: 'Saved.',
  erroGuardar: 'Could not save.',
}

const security = {
  titulo: 'Security',
  operadoresNota:
    'The platform administrators list is fixed by an environment variable (PLATFORM_ADMIN_USER_IDS). Managing operators from here is a later phase, not implemented yet.',
}

const communications = {
  titulo: 'Communications',
  texto: 'Administering FreeSWITCH, Kamailio and customer PBXs is a later phase of this console — there is no operator route for it yet.',
}

export default { ui, shell, auth, nodes, tenants, storage, loginSettings, security, communications }
