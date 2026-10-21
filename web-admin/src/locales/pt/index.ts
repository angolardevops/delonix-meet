// Dicionário PT-AO do backoffice de operador — pequeno de propósito: só as
// cinco secções que esta app tem. Ver `web/src/locales/pt/` para o padrão
// (um ficheiro por área), que aqui não se replica porque a área É o
// dicionário inteiro.

const ui = {
  erroCarregar: 'Não foi possível carregar.',
  tentarDeNovo: 'Tentar de novo',
  cancelar: 'Cancelar',
  confirmar: 'Confirmar',
  fechar: 'Fechar',
  guardar: 'Guardar',
  carregarMais: 'Carregar mais',
  rotaDesconhecida: {
    titulo: 'Página não encontrada',
    texto: 'Este endereço não corresponde a nenhuma secção do backoffice.',
    inicio: 'Ir para a Visão geral',
  },
}

const shell = {
  nav: {
    overview: 'Visão geral',
    tenants: 'Organizações',
    integrations: 'Armazenamento',
    security: 'Segurança',
    communications: 'Comunicações',
  },
  marca: 'Administração',
  terminarSessao: 'Terminar sessão',
  saltarParaConteudo: 'Saltar para o conteúdo',
  abrirNavegacao: 'Abrir navegação',
}

const auth = {
  titulo: 'Entrar',
  subtitulo: 'Administração da plataforma Delonix Meet',
  email: 'Email',
  password: 'Palavra-passe',
  entrar: 'Entrar',
  erro: 'Não foi possível entrar.',
}

const nodes = {
  titulo: 'Visão geral',
  semAcesso: {
    titulo: 'Sem acesso de operador',
    texto: 'A tua conta não está na lista de administradores da plataforma. Pede a um operador existente para te acrescentar a PLATFORM_ADMIN_USER_IDS.',
  },
  semSuperficie: {
    titulo: 'Sem superfície de operador',
    texto: 'Esta instalação não expõe uma superfície de operador.',
  },
  resumo: {
    serving: 'A servir',
    draining: 'A drenar',
    unreachable: 'Inalcançáveis',
    peers: 'Participantes',
  },
  tabela: {
    hostname: 'Nó',
    versao: 'Versão',
    estado: 'Estado',
    carga: 'Carga',
    salas: 'Salas',
    participantes: 'Participantes',
    visto: 'Visto pela última vez',
  },
  estado: {
    serving: 'a servir',
    draining: 'a drenar',
    unreachable: 'inalcançável',
  },
  vazio: 'Nenhum nó de media registado.',
}

const tenants = {
  titulo: 'Organizações',
  tabela: {
    nome: 'Nome',
    dominio: 'Domínio',
    membros: 'Membros',
    grupos: 'Grupos',
    salas: 'Salas',
    reunioes: 'Reuniões',
    armazenamento: 'Armazenamento',
    lugares: 'Lugares',
    participantes: 'Participantes',
  },
  ilimitado: 'ilimitado',
  vazio: 'Nenhuma organização encontrada.',
  detalhe: {
    titulo: 'Detalhe da organização',
    fechar: 'Fechar detalhe',
    uso: 'Uso actual',
    lugaresUsados: '{{used}} usados · {{active}} activos este mês',
    armazenamentoUsado: 'usado',
  },
  quotas: {
    titulo: 'Quotas do plano',
    grupos: 'Máx. grupos',
    salas: 'Máx. salas',
    reunioes: 'Máx. reuniões',
    vozBackend: 'Motor de voz',
    vozDid: 'Modelo de número',
    manterActual: '(manter actual)',
    backend: { freeswitch: 'FreeSWITCH', provider: 'Operadora' },
    did: { shared: 'Partilhado', dedicated: 'Dedicado' },
  },
  seats: {
    titulo: 'Lugares',
    max: 'Tecto de lugares',
  },
  concurrency: {
    titulo: 'Concorrência',
    max: 'Participantes concorrentes por nó',
  },
  guardado: 'Guardado.',
  erroGuardar: 'Não foi possível guardar.',
}

const storage = {
  titulo: 'Armazenamento das gravações',
  sub: 'Destino de armazenamento da plataforma — não da organização.',
  soPlataforma: 'Só a plataforma pode ver e alterar isto.',
  destino: 'Destino',
  tipo: { local: 'Local', nfs: 'NFS', webdav: 'WebDAV' },
  tipoDica: {
    local: 'Disco do nó — sem redundância entre nós.',
    nfs: 'Partilha NFS montada pelos nós de media.',
    webdav: 'Servidor WebDAV externo.',
  },
  emUso: 'em uso',
  nfsDica: 'O nó monta esta partilha antes de escrever gravações.',
  nfsServidor: 'Servidor NFS',
  nfsCaminho: 'Caminho',
  manifesto: 'Transferir manifesto PVC',
  manifestoGuardarPrimeiro: 'Grava primeiro para poderes transferir o manifesto.',
  manifestoFalhou: 'Não foi possível obter o manifesto.',
  webdavDica: 'Credenciais guardadas cifradas; a palavra-passe não se volta a mostrar.',
  webdavUrl: 'URL',
  webdavUtilizador: 'Utilizador',
  webdavPassword: 'Palavra-passe',
  passwordDefinida: 'Já tem palavra-passe definida.',
  webdavCaminho: 'Caminho',
  objectosTitulo: 'Armazenamento de objectos',
  objectosSub: 'Configurado no servidor, mas ainda não usado pelas gravações.',
  objectosEndpoint: 'Destino',
  objectosBucket: 'Balde',
  objectosAindaNao: 'As gravações continuam em disco (ADR-0020) — este balde ainda não as recebe.',
  testar: 'Testar',
  testeOk: 'Ligação feita.',
  testeFalhou: 'A ligação falhou.',
  testaGravado: 'Grava para poderes testar o destino novo.',
  guardado: 'Guardado.',
  erroGuardar: 'Não foi possível guardar.',
}

const loginSettings = {
  titulo: 'Entrada na plataforma',
  hideOrgCreation: 'Esconder criação de organização',
  hideOrgCreationHint: 'O ecrã de entrada deixa de oferecer "criar organização nova".',
  hideSsoButton: 'Esconder botão de SSO',
  hideSsoButtonHint: 'O ecrã de entrada deixa de mostrar o atalho de SSO por domínio.',
  guardado: 'Guardado.',
  erroGuardar: 'Não foi possível guardar.',
}

const security = {
  titulo: 'Segurança',
  operadoresNota:
    'A lista de administradores de plataforma é fixa por variável de ambiente (PLATFORM_ADMIN_USER_IDS). Gerir operadores a partir daqui é uma fase posterior, ainda não implementada.',
}

const communications = {
  titulo: 'Comunicações',
  texto:
    'A administração de FreeSWITCH, Kamailio e dos PBX de cliente é uma fase posterior desta consola — ainda não existe rota de operador para isto.',
}

export default { ui, shell, auth, nodes, tenants, storage, loginSettings, security, communications }
