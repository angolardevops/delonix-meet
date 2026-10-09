import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import '../conta/conta_sip.dart';
import 'digest.dart';

/// O que a app mostra e o utilizador percebe; o motivo técnico fica em [codigo].
class ResultadoRegisto {
  const ResultadoRegisto.registado() : this._(true, 200, 'Registado');
  const ResultadoRegisto.falhou(int? codigo, String mensagem)
    : this._(false, codigo, mensagem);
  const ResultadoRegisto._(this.ok, this.codigo, this.mensagem);

  final bool ok;
  final int? codigo;
  final String mensagem;
}

/// A porta do registo: a UI e o controlador só conhecem isto. O motor SIP definitivo (ainda por
/// escolher, ver o documento de requisitos §4) ocupará este lugar.
abstract interface class ServicoRegisto {
  Future<ResultadoRegisto> registar(
    ContaSip conta, {
    int expiraSegundos = 3600,
  });
  Future<void> desregistar(ContaSip conta);
}

/// REGISTER com digest sobre UDP ou TLS, em Dart puro.
///
/// **É uma ferramenta de diagnóstico, não o motor de chamadas.** Prova que as credenciais
/// provisionadas registam no FreeSWITCH e dá o estado de registo (RF-07). TLS confere o
/// certificado do servidor contra as raízes do sistema (e, só fora de release, contra
/// [raizConfiavel], a raiz do laboratório). TCP em claro não existe. Sem SRTP, sem chamadas, sem
/// NAT/CGNAT (RNF-45): isso é do motor.
class RegistoSip implements ServicoRegisto {
  RegistoSip({
    this.t1 = const Duration(milliseconds: 500),
    this.prazo = const Duration(seconds: 10),
    List<int>? raizConfiavel,
    bool release = false,
    Random? aleatorio,
  }) : _raiz = release ? null : raizConfiavel,
       _r = aleatorio ?? Random.secure();

  final Duration t1;
  final Duration prazo;
  final List<int>? _raiz;
  final Random _r;

  @override
  Future<ResultadoRegisto> registar(
    ContaSip conta, {
    int expiraSegundos = 3600,
  }) => _registo(conta, expiraSegundos);

  @override
  Future<void> desregistar(ContaSip conta) async {
    await _registo(conta, 0);
  }

  String _hex(int bytes) => List.generate(
    bytes,
    (_) => _r.nextInt(256).toRadixString(16).padLeft(2, '0'),
  ).join();

  Future<ResultadoRegisto> _registo(ContaSip conta, int expira) async {
    if (conta.servidor.transporte == TransporteSip.tcp) {
      return const ResultadoRegisto.falhou(
        null,
        'TCP em claro não é suportado: use TLS.',
      );
    }
    final _Canal canal;
    try {
      canal = conta.servidor.transporte == TransporteSip.tls
          ? await _CanalTls.abrir(conta.servidor, _raiz, prazo)
          : await _CanalUdp.abrir(conta.servidor, prazo, t1);
    } on HandshakeException catch (e) {
      // `e.message` e o erro do SO descrevem o handshake (certificado, versão, ligação cortada);
      // não levam credenciais.
      return ResultadoRegisto.falhou(
        null,
        'O certificado do servidor SIP não é de confiança (${e.message}${e.osError == null ? '' : ': ${e.osError!.message}'}).',
      );
    } on _SemServidor {
      return const ResultadoRegisto.falhou(
        null,
        'Não foi possível resolver o servidor.',
      );
    } on SocketException {
      return const ResultadoRegisto.falhou(
        null,
        'Não foi possível ligar ao servidor SIP.',
      );
    } on TimeoutException {
      return const ResultadoRegisto.falhou(
        null,
        'O servidor SIP não respondeu a tempo.',
      );
    }
    final callId = '${_hex(8)}@delonixphone';
    final tag = _hex(4);
    var cseq = 0;
    final uriRegisto = 'sip:${conta.dominio}';
    final identidade = 'sip:${conta.utilizador}@${conta.dominio}';
    final nome = conta.nomeExibicao.replaceAll(RegExp(r'["\\<>\r\n]'), '');

    String pedido(String? autorizacao) => [
      'REGISTER $uriRegisto SIP/2.0',
      'Via: SIP/2.0/${canal.via} ${canal.ipLocal}:${canal.portaLocal};branch=z9hG4bK${_hex(6)};rport',
      'Max-Forwards: 70',
      'From: "$nome" <$identidade>;tag=$tag',
      'To: <$identidade>',
      'Call-ID: $callId',
      'CSeq: ${++cseq} REGISTER',
      'Contact: <sip:${conta.utilizador}@${canal.ipLocal}:${canal.portaLocal};transport=${canal.via.toLowerCase()}>',
      'Expires: $expira',
      'User-Agent: DelonixPhone/0.1 (diagnostico)',
      if (autorizacao != null) 'Authorization: $autorizacao',
      'Content-Length: 0',
      '',
      '',
    ].join('\r\n');

    try {
      final p1 = pedido(null);
      var resp = await _transaccao(canal, p1, callId, cseq);
      if (resp == null) {
        return const ResultadoRegisto.falhou(null, 'O servidor não respondeu.');
      }
      if (resp.codigo == 401 || resp.codigo == 407) {
        final desafio = DesafioDigest.ler(
          resp.cabecalho(
                resp.codigo == 401 ? 'www-authenticate' : 'proxy-authenticate',
              ) ??
              '',
        );
        if (desafio == null) {
          return const ResultadoRegisto.falhou(
            401,
            'O servidor pediu uma autenticação que a app não entende.',
          );
        }
        if (desafio.algoritmo != 'MD5') {
          return ResultadoRegisto.falhou(
            401,
            'Algoritmo de autenticação ${desafio.algoritmo} não suportado.',
          );
        }
        final cnonce = _hex(8);
        final r = respostaDigest(
          utilizador: conta.utilizador,
          realm: desafio.realm,
          palavraPasse: conta.palavraPasse,
          metodo: 'REGISTER',
          uri: uriRegisto,
          nonce: desafio.nonce,
          qop: desafio.qop,
          cnonce: cnonce,
        );
        final auth =
            'Digest username="${conta.utilizador}", realm="${desafio.realm}", nonce="${desafio.nonce}", '
            'uri="$uriRegisto", response="$r", algorithm=MD5'
            '${desafio.qop != null ? ', qop=auth, nc=00000001, cnonce="$cnonce"' : ''}'
            '${desafio.opaque != null ? ', opaque="${desafio.opaque}"' : ''}';
        final p2 = pedido(auth);
        resp = await _transaccao(canal, p2, callId, cseq);
        if (resp == null) {
          return const ResultadoRegisto.falhou(
            null,
            'O servidor não respondeu.',
          );
        }
      }
      return switch (resp.codigo) {
        200 => const ResultadoRegisto.registado(),
        401 || 403 => ResultadoRegisto.falhou(
          resp.codigo,
          'Registo recusado: credenciais inválidas (${resp.codigo}).',
        ),
        final c => ResultadoRegisto.falhou(
          c,
          'Registo falhou ($c ${resp.motivo}).',
        ),
      };
    } finally {
      await canal.fechar();
    }
  }

  /// Envia e espera a resposta final com a mesma Call-ID e CSeq: um 401 atrasado da 1.ª tentativa
  /// não pode fechar a 2.ª. Em UDP reenvia com T1 a duplicar (RFC 3261 §17.1.2.2); em TLS o
  /// transporte já é fiável e não se reenvia.
  Future<_Resposta?> _transaccao(
    _Canal c,
    String msg,
    String callId,
    int cseq,
  ) async {
    final fim = DateTime.now().add(prazo);
    final pronta = Completer<_Resposta?>();
    final sub = c.mensagens.listen((t) {
      final r = _Resposta.ler(t);
      final mesmo =
          r != null &&
          r.cabecalho('call-id') == callId &&
          r.cabecalho('cseq')?.startsWith('$cseq ') == true;
      if (mesmo && r.codigo >= 200 && !pronta.isCompleted) pronta.complete(r);
    });
    var espera = t1;
    () async {
      while (!pronta.isCompleted && DateTime.now().isBefore(fim)) {
        c.enviar(msg);
        if (c.fiavel) {
          await pronta.future.timeout(
            fim.difference(DateTime.now()),
            onTimeout: () => null,
          );
          break;
        }
        await Future<void>.delayed(espera);
        espera = Duration(milliseconds: min(espera.inMilliseconds * 2, 4000));
      }
      if (!pronta.isCompleted) pronta.complete(null);
    }();
    try {
      return await pronta.future;
    } finally {
      await sub.cancel();
    }
  }
}

class _SemServidor implements Exception {}

/// O canal por onde vão os pedidos: datagramas (UDP) ou um fluxo cifrado (TLS).
abstract class _Canal {
  String get via;
  String get ipLocal;
  int get portaLocal;
  bool get fiavel;
  Stream<String> get mensagens;
  void enviar(String mensagem);
  Future<void> fechar();
}

/// O IPv4 local que sai por uma interface que não é loopback (para o Via/Contact).
Future<String> _ipLocal() async {
  for (final i in await NetworkInterface.list(type: InternetAddressType.IPv4)) {
    for (final a in i.addresses) {
      if (!a.isLoopback) return a.address;
    }
  }
  return '0.0.0.0';
}

class _CanalUdp implements _Canal {
  _CanalUdp._(this._socket, this._destino, this._porta, this.ipLocal) {
    _socket.listen((e) {
      if (e == RawSocketEvent.read) {
        final d = _socket.receive();
        if (d != null) _entrada.add(utf8.decode(d.data, allowMalformed: true));
      }
    });
  }

  final RawDatagramSocket _socket;
  final InternetAddress _destino;
  final int _porta;
  final _entrada = StreamController<String>.broadcast();

  static Future<_CanalUdp> abrir(
    ServidorSip s,
    Duration prazo,
    Duration t1,
  ) async {
    final enderecos = await InternetAddress.lookup(
      s.anfitriao,
      type: InternetAddressType.IPv4,
    ).timeout(prazo, onTimeout: () => <InternetAddress>[]);
    if (enderecos.isEmpty) throw _SemServidor();
    final socket = await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    return _CanalUdp._(socket, enderecos.first, s.porta, await _ipLocal());
  }

  @override
  final String ipLocal;
  @override
  String get via => 'UDP';
  @override
  int get portaLocal => _socket.port;
  @override
  bool get fiavel => false;
  @override
  Stream<String> get mensagens => _entrada.stream;
  @override
  void enviar(String m) => _socket.send(utf8.encode(m), _destino, _porta);
  @override
  Future<void> fechar() async {
    _socket.close();
    await _entrada.close();
  }
}

class _CanalTls implements _Canal {
  _CanalTls._(this._socket) {
    _socket.listen(
      (bytes) {
        _buffer.addAll(bytes);
        _cortar();
      },
      onDone: () => _entrada.close(),
      onError: (Object _) => _entrada.close(),
      cancelOnError: true,
    );
  }

  final SecureSocket _socket;
  final _buffer = <int>[];
  final _entrada = StreamController<String>.broadcast();

  static Future<_CanalTls> abrir(
    ServidorSip s,
    List<int>? raiz,
    Duration prazo,
  ) async {
    final ctx = SecurityContext(withTrustedRoots: true);
    if (raiz != null && raiz.isNotEmpty) ctx.setTrustedCertificatesBytes(raiz);
    // O certificado é conferido contra o nome/IP a que nos ligamos; sem `onBadCertificate`, uma
    // falha é um HandshakeException e a ligação não se faz.
    return _CanalTls._(
      await SecureSocket.connect(
        s.anfitriao,
        s.porta,
        context: ctx,
        timeout: prazo,
      ),
    );
  }

  /// Corta o fluxo em mensagens SIP: cabeçalhos até à linha vazia, mais `Content-Length` bytes.
  void _cortar() {
    while (true) {
      final fim = _procurar(_buffer, const [13, 10, 13, 10]);
      if (fim < 0) return;
      final cab = utf8.decode(_buffer.sublist(0, fim), allowMalformed: true);
      final cl = int.tryParse(
        RegExp(
              r'^(?:content-length|l)\s*:\s*(\d+)',
              caseSensitive: false,
              multiLine: true,
            ).firstMatch(cab)?.group(1) ??
            '0',
      )!;
      if (_buffer.length < fim + 4 + cl) return;
      _buffer.removeRange(0, fim + 4 + cl);
      _entrada.add('$cab\r\n\r\n');
    }
  }

  static int _procurar(List<int> dados, List<int> agulha) {
    for (var i = 0; i + agulha.length <= dados.length; i++) {
      var igual = true;
      for (var j = 0; j < agulha.length; j++) {
        if (dados[i + j] != agulha[j]) {
          igual = false;
          break;
        }
      }
      if (igual) return i;
    }
    return -1;
  }

  @override
  String get via => 'TLS';
  @override
  String get ipLocal => _socket.address.address;
  @override
  int get portaLocal => _socket.port;
  @override
  bool get fiavel => true;
  @override
  Stream<String> get mensagens => _entrada.stream;
  @override
  void enviar(String m) => _socket.add(utf8.encode(m));
  @override
  Future<void> fechar() async {
    await _socket.close();
    if (!_entrada.isClosed) await _entrada.close();
  }
}

class _Resposta {
  _Resposta(this.codigo, this.motivo, this._cab);
  final int codigo;
  final String motivo;
  final Map<String, String> _cab;
  String? cabecalho(String nome) => _cab[nome.toLowerCase()];

  static _Resposta? ler(String t) {
    final linhas = t.split('\r\n');
    final m = RegExp(r'^SIP/2\.0 (\d{3}) ?(.*)$').firstMatch(linhas.first);
    if (m == null) return null;
    final cab = <String, String>{};
    for (final l in linhas.skip(1)) {
      if (l.isEmpty) break;
      final i = l.indexOf(':');
      if (i > 0) {
        cab.putIfAbsent(
          l.substring(0, i).trim().toLowerCase(),
          () => l.substring(i + 1).trim(),
        );
      }
    }
    return _Resposta(int.parse(m.group(1)!), m.group(2)!, cab);
  }
}
