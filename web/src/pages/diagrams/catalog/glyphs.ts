/**
 * Glifos genéricos do catálogo de arquitectura, desenhados por nós (viewBox
 * 24×24, só traço). NÃO são ícones de fornecedores — ver
 * `docs/quadros/quadros-formas-licencas.md`: AWS, Azure e Google Cloud não
 * permitem redistribuir os ícones oficiais dentro de uma aplicação, por isso o
 * cartão mostra o TIPO de recurso com a cor do fornecedor e o nome do serviço.
 *
 * Vive num módulo carregado com os grupos (import dinâmico), fora do bundle
 * inicial.
 */
export const GLYPHS = {
  compute: 'M6 6h12v12H6zM9 9h6v6H9zM9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3',
  server: 'M4 4h16v6H4zM4 14h16v6H4zM7 7h.01M7 17h.01M11 7h6M11 17h6',
  container: 'M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12 4 7.5',
  serverless: 'M7 4h3l7 16M12.5 11 7 20',
  objectStorage: 'M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zM4 6l2 13c.3 1.2 2.9 2 6 2s5.7-.8 6-2l2-13',
  blockStorage: 'M4 5h16v14H4zM4 10h16M8 14.5h.01M12 14.5h4',
  database: 'M5 6c0-1.7 3.1-3 7-3s7 1.3 7 3v12c0 1.7-3.1 3-7 3s-7-1.3-7-3zM5 6c0 1.7 3.1 3 7 3s7-1.3 7-3M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3',
  document: 'M6 3h9l4 4v14H6zM15 3v4h4M9 12h7M9 16h7',
  cache: 'M5 6c0-1.7 3.1-3 7-3s7 1.3 7 3v12c0 1.7-3.1 3-7 3s-7-1.3-7-3zM5 6c0 1.7 3.1 3 7 3s7-1.3 7-3M13 10l-3 5h4l-3 5',
  queue: 'M3 7h18v10H3zM8 7v10M13 7v10',
  stream: 'M3 8c3-3 6 3 9 0s6 3 9 0M3 13c3-3 6 3 9 0s6 3 9 0M3 18c3-3 6 3 9 0s6 3 9 0',
  topic: 'M10 12a2 2 0 1 0 4 0 2 2 0 0 0-4 0M3 5h4v4H3zM17 5h4v4h-4zM3 15h4v4H3zM17 15h4v4h-4zM7 8l3.3 2.8M17 8l-3.3 2.8M7 16l3.3-2.8M17 16l-3.3-2.8',
  globe: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18',
  loadBalancer: 'M12 3v6M12 9 5 15M12 9l7 6M12 9v6M3 15h4v4H3zM10 15h4v4h-4zM17 15h4v4h-4z',
  apiGateway: 'M8 4H6a2 2 0 0 0-2 2v4l-2 2 2 2v4a2 2 0 0 0 2 2h2M16 4h2a2 2 0 0 1 2 2v4l2 2-2 2v4a2 2 0 0 1-2 2h-2M9 12h6',
  dns: 'M12 3v18M5 5h11l3 3-3 3H5zM19 13H8l-3 3 3 3h11z',
  cloud: 'M7 18h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 9a4.5 4.5 0 0 0 1 9z',
  subnet: 'M4 4h16v16H4zM4 12h16M12 4v16',
  region: 'M12 21s-7-6.4-7-12a7 7 0 0 1 14 0c0 5.6-7 12-7 12zM9.5 9a2.5 2.5 0 1 0 5 0 2.5 2.5 0 0 0-5 0',
  firewall: 'M3 5h18v14H3zM3 9.7h18M3 14.3h18M9 5v4.7M15 5v4.7M6 9.7v4.6M12 9.7v4.6M18 9.7v4.6M9 14.3V19M15 14.3V19',
  shield: 'M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM9 12l2 2 4-4',
  identity: 'M9 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM3 20a6 6 0 0 1 12 0M16 10h5M18.5 7.5v5',
  monitoring: 'M3 3v18h18M7 15l4-5 3 3 5-7',
  secret: 'M8 15a4 4 0 1 1 3.5-6H21v3h-2v3h-3v-3h-4.5A4 4 0 0 1 8 15z',
  router: 'M4 14h16v6H4zM8 17h.01M12 17h.01M8 14 6 10M16 14l2-4M9 7a4 4 0 0 1 6 0M7 4.5a7 7 0 0 1 10 0',
  switch: 'M3 8h18v8H3zM7 12h.01M10 12h.01M13 12h.01M16 12h.01M6 5h4M14 19h4',
  vpn: 'M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3M12 15v2',
  proxy: 'M4 12h6M14 12h6M17 9l3 3-3 3M10 7h4v10h-4z',
  rack: 'M5 3h14v18H5zM5 8h14M5 13h14M8 5.5h.01M8 10.5h.01M8 15.5h.01',
  vm: 'M3 4h18v12H3zM8 20h8M12 16v4M8 8h8v4H8z',
  nas: 'M4 4h16v16H4zM8 8h8M8 12h8M8 16h.01M11 16h.01',
  hypervisor: 'M3 17h18v4H3zM5 4h6v6H5zM13 4h6v6h-6zM8 10v7M16 10v7',
  workstation: 'M3 4h18v12H3zM8 20h8M12 16v4',
  backup: 'M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4M12 8v4l3 2',
  analytics: 'M4 11a7 7 0 1 0 14 0 7 7 0 0 0-14 0M16 16l5 5M8 13v-2M11 13V8M14 13v-3',
  web: 'M3 4h18v16H3zM3 8h18M6 6h.01M9 6h.01',
  notification: 'M6 17v-6a6 6 0 1 1 12 0v6l2 2H4zM10 21h4',
  registry: 'M4 7l8-4 8 4v10l-8 4-8-4zM8 9l8 4M12 11v8',
  cluster: 'M12 3l4 2.3v4.6L12 12 8 9.9V5.3zM7 12l4 2.3v4.6L7 21l-4-2.1v-4.6zM17 12l4 2.3v4.6L17 21l-4-2.1v-4.6z',
  video: 'M3 7h12v10H3zM15 10.5 21 7v10l-6-3.5',
  app: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
  vector: 'M4 20 20 4M4 20h6M4 20v-6M9 20l11-7M4 15l7-11',
  mail: 'M3 5h18v14H3zM3 5l9 7 9-7',
  ai: 'M12 3v3M12 18v3M3 12h3M18 12h3M7 7h10v10H7zM10 10h4v4h-4z',
} as const

export type GlyphName = keyof typeof GLYPHS
