enum TransporteSip {
  udp,
  tcp,
  tls;

  /// Só o TLS cifra a sinalização. As chaves do SRTP (SDES) viajam no SDP, por isso sem TLS
  /// seguem em claro (ADR-0009): fora do laboratório, a app recusa o resto (RNF-20).
  bool get cifrado => this == tls;

  static TransporteSip doNome(String nome) => switch (nome.toLowerCase()) {
    'udp' => udp,
    'tcp' => tcp,
    'tls' => tls,
    _ => throw FormatException('transporte SIP desconhecido', nome),
  };
}

/// Onde o aparelho se liga de facto (o proxy público do ramal). Não é o domínio da conta:
/// esse é o realm do digest, um nome lógico que não resolve em DNS.
class ServidorSip {
  const ServidorSip({
    required this.anfitriao,
    required this.porta,
    required this.transporte,
  });

  final String anfitriao;
  final int porta;
  final TransporteSip transporte;

  /// `sip:host:porta;transport=x`, o mesmo formato que o servidor devolve em `sip_server.uri`.
  String get uri => 'sip:$anfitriao:$porta;transport=${transporte.name}';

  /// Lê `sip:host[:porta][;transport=x]`, com ou sem `<…>`. Sem porta, 5060 (5061 em TLS).
  static ServidorSip doUri(String texto) {
    final limpo = texto.trim().replaceAll(RegExp(r'^<|>$'), '');
    final m = RegExp(
      r'^sips?:([^:;>\s]+)(?::(\d+))?((?:;[^\s]*)?)$',
      caseSensitive: false,
    ).firstMatch(limpo);
    if (m == null) {
      throw FormatException('endereço de servidor SIP inválido', texto);
    }
    var transporte = TransporteSip.udp;
    for (final p in m.group(3)!.split(';')) {
      if (p.toLowerCase().startsWith('transport=')) {
        transporte = TransporteSip.doNome(p.substring(10));
      }
    }
    if (limpo.toLowerCase().startsWith('sips:')) transporte = TransporteSip.tls;
    final porta =
        int.tryParse(m.group(2) ?? '') ??
        (transporte == TransporteSip.tls ? 5061 : 5060);
    if (porta < 1 || porta > 65535) {
      throw FormatException('porta fora do intervalo', texto);
    }
    return ServidorSip(
      anfitriao: m.group(1)!,
      porta: porta,
      transporte: transporte,
    );
  }

  Map<String, Object> toJson() => {
    'anfitriao': anfitriao,
    'porta': porta,
    'transporte': transporte.name,
  };

  static ServidorSip fromJson(Map<String, dynamic> j) => ServidorSip(
    anfitriao: j['anfitriao'] as String,
    porta: j['porta'] as int,
    transporte: TransporteSip.doNome(j['transporte'] as String),
  );

  @override
  bool operator ==(Object other) =>
      other is ServidorSip &&
      other.anfitriao == anfitriao &&
      other.porta == porta &&
      other.transporte == transporte;
  @override
  int get hashCode => Object.hash(anfitriao, porta, transporte);
}

/// Uma conta SIP (um ramal). A palavra-passe NUNCA entra em `toString`, em registos nem em
/// mensagens de erro (RNF-23): só o armazém seguro a guarda.
class ContaSip {
  const ContaSip({
    required this.nomeExibicao,
    required this.utilizador,
    required this.palavraPasse,
    required this.dominio,
    required this.servidor,
    this.srtpObrigatorio = true,
  });

  final String nomeExibicao;
  final String utilizador;
  final String palavraPasse;

  /// O realm do digest.
  final String dominio;
  final ServidorSip servidor;
  final bool srtpObrigatorio;

  Map<String, Object> toJson() => {
    'nomeExibicao': nomeExibicao,
    'utilizador': utilizador,
    'palavraPasse': palavraPasse,
    'dominio': dominio,
    'servidor': servidor.toJson(),
    'srtpObrigatorio': srtpObrigatorio,
  };

  static ContaSip fromJson(Map<String, dynamic> j) => ContaSip(
    nomeExibicao: j['nomeExibicao'] as String,
    utilizador: j['utilizador'] as String,
    palavraPasse: j['palavraPasse'] as String,
    dominio: j['dominio'] as String,
    servidor: ServidorSip.fromJson(j['servidor'] as Map<String, dynamic>),
    srtpObrigatorio: j['srtpObrigatorio'] as bool? ?? true,
  );

  @override
  String toString() => 'ContaSip($utilizador@$dominio via ${servidor.uri})';
}
