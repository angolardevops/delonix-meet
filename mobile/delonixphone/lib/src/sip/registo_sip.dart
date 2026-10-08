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

/// REGISTER com digest sobre UDP, em Dart puro.
///
/// **É uma ferramenta de diagnóstico, não o motor de chamadas.** Prova que as credenciais
/// provisionadas registam no FreeSWITCH e dá o estado de registo (RF-07). Só faz UDP: TCP e TLS
/// ainda não (o laboratório só expõe UDP em 5070), por isso devolve uma falha clara em vez de
/// fingir. Sem SRTP, sem chamadas, sem NAT/CGNAT (RNF-45): isso é do motor.
class RegistoSipUdp implements ServicoRegisto {
  RegistoSipUdp({
    this.t1 = const Duration(milliseconds: 500),
    this.prazo = const Duration(seconds: 10),
    Random? aleatorio,
  }) : _r = aleatorio ?? Random.secure();

  final Duration t1;
  final Duration prazo;
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
    if (conta.servidor.transporte != TransporteSip.udp) {
      return ResultadoRegisto.falhou(
        null,
        'O transporte ${conta.servidor.transporte.name.toUpperCase()} ainda não é suportado nesta versão.',
      );
    }
    final enderecos = await InternetAddress.lookup(
      conta.servidor.anfitriao,
      type: InternetAddressType.IPv4,
    ).timeout(prazo, onTimeout: () => <InternetAddress>[]);
    if (enderecos.isEmpty) {
      return const ResultadoRegisto.falhou(
        null,
        'Não foi possível resolver o servidor.',
      );
    }
    final destino = enderecos.first;
    final socket = await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    final respostas = StreamController<String>.broadcast();
    socket.listen((e) {
      if (e == RawSocketEvent.read) {
        final d = socket.receive();
        if (d != null) respostas.add(utf8.decode(d.data, allowMalformed: true));
      }
    });
    final ip = await _ipLocal(destino);
    final callId = '${_hex(8)}@delonixphone';
    final tag = _hex(4);
    var cseq = 0;
    final uriRegisto = 'sip:${conta.dominio}';
    final identidade = 'sip:${conta.utilizador}@${conta.dominio}';
    final nome = conta.nomeExibicao.replaceAll(RegExp(r'["\\<>\r\n]'), '');

    String pedido(String? autorizacao) => [
      'REGISTER $uriRegisto SIP/2.0',
      'Via: SIP/2.0/UDP $ip:${socket.port};branch=z9hG4bK${_hex(6)};rport',
      'Max-Forwards: 70',
      'From: "$nome" <$identidade>;tag=$tag',
      'To: <$identidade>',
      'Call-ID: $callId',
      'CSeq: ${++cseq} REGISTER',
      'Contact: <sip:${conta.utilizador}@$ip:${socket.port};transport=udp>',
      'Expires: $expira',
      'User-Agent: DelonixPhone/0.1 (diagnostico)',
      if (autorizacao != null) 'Authorization: $autorizacao',
      'Content-Length: 0',
      '',
      '',
    ].join('\r\n');

    try {
      var resp = await _transaccao(
        socket,
        destino,
        conta.servidor.porta,
        pedido(null),
        respostas.stream,
        callId,
      );
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
        resp = await _transaccao(
          socket,
          destino,
          conta.servidor.porta,
          pedido(auth),
          respostas.stream,
          callId,
        );
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
          'Registo falhou (${resp.codigo} ${resp.motivo}).',
        ),
      };
    } finally {
      socket.close();
      await respostas.close();
    }
  }

  /// Enviar, e reenviar com T1 a duplicar (RFC 3261 §17.1.2.2) até haver resposta final.
  Future<_Resposta?> _transaccao(
    RawDatagramSocket s,
    InternetAddress d,
    int porta,
    String msg,
    Stream<String> entrada,
    String callId,
  ) async {
    final dados = utf8.encode(msg);
    final fim = DateTime.now().add(prazo);
    final c = Completer<_Resposta?>();
    final sub = entrada.listen((t) {
      final r = _Resposta.ler(t);
      if (r != null &&
          r.cabecalho('call-id') == callId &&
          r.codigo >= 200 &&
          !c.isCompleted) {
        c.complete(r);
      }
    });
    var espera = t1;
    () async {
      while (!c.isCompleted && DateTime.now().isBefore(fim)) {
        s.send(dados, d, porta);
        await Future<void>.delayed(espera);
        espera = Duration(milliseconds: min(espera.inMilliseconds * 2, 4000));
      }
      if (!c.isCompleted) c.complete(null);
    }();
    try {
      return await c.future;
    } finally {
      await sub.cancel();
    }
  }

  /// O IPv4 local que sai para [destino] (para o Via/Contact). Cai em 0.0.0.0 se não houver.
  Future<String> _ipLocal(InternetAddress destino) async {
    for (final i in await NetworkInterface.list(
      type: InternetAddressType.IPv4,
    )) {
      for (final a in i.addresses) {
        if (!a.isLoopback) return a.address;
      }
    }
    return '0.0.0.0';
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
