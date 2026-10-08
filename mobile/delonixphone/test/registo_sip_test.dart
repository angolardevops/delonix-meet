import 'dart:convert';
import 'dart:io';

import 'package:delonixphone/src/conta/conta_sip.dart';
import 'package:delonixphone/src/sip/digest.dart';
import 'package:delonixphone/src/sip/registo_sip.dart';
import 'package:flutter_test/flutter_test.dart';

/// Um «FreeSWITCH» mínimo: 401 com desafio e depois 200 ou 403 conforme o digest bata certo.
class _Servidor {
  _Servidor(
    this.socket,
    this.palavraPasse, {
    this.qop = true,
    this.ignorar = false,
  }) {
    socket.listen((e) {
      if (e != RawSocketEvent.read) return;
      final d = socket.receive();
      if (d == null || ignorar) return;
      _atender(utf8.decode(d.data), d);
    });
  }
  final RawDatagramSocket socket;
  final String palavraPasse;
  final bool qop;
  final bool ignorar;
  final pedidos = <String>[];
  static const realm = 'org.ramais.delonix.meet';
  static const nonce = 'abc123nonce';

  static Future<_Servidor> iniciar(
    String palavraPasse, {
    bool qop = true,
    bool ignorar = false,
  }) async => _Servidor(
    await RawDatagramSocket.bind(InternetAddress.loopbackIPv4, 0),
    palavraPasse,
    qop: qop,
    ignorar: ignorar,
  );

  String _h(String msg, String nome) =>
      RegExp(
        '^$nome:\\s*(.*)\$',
        multiLine: true,
        caseSensitive: false,
      ).firstMatch(msg)?.group(1)?.trim() ??
      '';

  void _atender(String msg, Datagram d) {
    pedidos.add(msg);
    final aut = _h(msg, 'Authorization');
    String estado;
    final extra = <String>[];
    if (aut.isEmpty) {
      estado = '401 Unauthorized';
      extra.add(
        'WWW-Authenticate: Digest realm="$realm", nonce="$nonce", algorithm=MD5${qop ? ', qop="auth"' : ''}',
      );
    } else {
      final cnonce =
          RegExp(r'cnonce="([^"]*)"').firstMatch(aut)?.group(1) ?? '';
      final resp = RegExp(r'response="([^"]*)"').firstMatch(aut)!.group(1);
      final esperado = respostaDigest(
        utilizador: 'ramal_x',
        realm: realm,
        palavraPasse: palavraPasse,
        metodo: 'REGISTER',
        uri: 'sip:$realm',
        nonce: nonce,
        qop: qop ? 'auth' : null,
        cnonce: cnonce,
      );
      estado = resp == esperado ? '200 OK' : '403 Forbidden';
    }
    final r = [
      'SIP/2.0 $estado',
      'Via: ${_h(msg, 'Via')}',
      'From: ${_h(msg, 'From')}',
      'To: ${_h(msg, 'To')};tag=srv',
      'Call-ID: ${_h(msg, 'Call-ID')}',
      'CSeq: ${_h(msg, 'CSeq')}',
      ...extra,
      'Content-Length: 0',
      '',
      '',
    ].join('\r\n');
    socket.send(utf8.encode(r), d.address, d.port);
  }

  void fechar() => socket.close();
}

ContaSip _conta(
  _Servidor s, {
  String palavraPasse = 'segredo',
  TransporteSip t = TransporteSip.udp,
}) => ContaSip(
  nomeExibicao: 'Ana',
  utilizador: 'ramal_x',
  palavraPasse: palavraPasse,
  dominio: _Servidor.realm,
  servidor: ServidorSip(
    anfitriao: '127.0.0.1',
    porta: s.socket.port,
    transporte: t,
  ),
);

void main() {
  group('respostaDigest', () {
    test('vector do RFC 2617 §3.5', () {
      expect(
        respostaDigest(
          utilizador: 'Mufasa',
          realm: 'testrealm@host.com',
          palavraPasse: 'Circle Of Life',
          metodo: 'GET',
          uri: '/dir/index.html',
          nonce: 'dcd98b7102dd2f0e8b11d0f600bfb0c093',
          qop: 'auth',
          nc: '00000001',
          cnonce: '0a4f113b',
        ),
        '6629fae49393a05397450978507c4ef1',
      );
    });
    test('sem qop usa a forma antiga do RFC 2069', () {
      expect(
        respostaDigest(
          utilizador: 'Mufasa',
          realm: 'testrealm@host.com',
          palavraPasse: 'Circle Of Life',
          metodo: 'GET',
          uri: '/dir/index.html',
          nonce: 'dcd98b7102dd2f0e8b11d0f600bfb0c093',
        ),
        '670fd8c2df070c60b045671b8b24ff02',
      );
    });
  });

  group('DesafioDigest.ler', () {
    test('lê aspas, opaque e escolhe auth entre auth,auth-int', () {
      final d = DesafioDigest.ler(
        'Digest realm="r", nonce="n", opaque="o", algorithm=MD5, qop="auth,auth-int"',
      )!;
      expect(
        (d.realm, d.nonce, d.opaque, d.qop, d.algoritmo),
        ('r', 'n', 'o', 'auth', 'MD5'),
      );
    });
    test('não é Digest, ou falta o nonce', () {
      expect(DesafioDigest.ler('Basic realm="x"'), isNull);
      expect(DesafioDigest.ler('Digest realm="x"'), isNull);
    });
  });

  group('RegistoSipUdp contra um servidor simulado', () {
    late _Servidor srv;
    tearDown(() => srv.fechar());
    final registo = RegistoSipUdp(
      t1: const Duration(milliseconds: 50),
      prazo: const Duration(seconds: 2),
    );

    test('desafio 401 e depois 200: registado', () async {
      srv = await _Servidor.iniciar('segredo');
      final r = await registo.registar(_conta(srv));
      expect(r.ok, isTrue, reason: r.mensagem);
      expect(srv.pedidos, hasLength(2));
      expect(
        srv.pedidos.first,
        startsWith('REGISTER sip:org.ramais.delonix.meet SIP/2.0\r\n'),
      );
      expect(srv.pedidos.first, contains('Call-ID: '));
      // A mesma Call-ID e CSeq a subir, como manda o RFC 3261 §22.2.
      String h(String m, String n) => RegExp(
        '^$n: (.*)\$',
        multiLine: true,
      ).firstMatch(m)!.group(1)!.trim();
      expect(h(srv.pedidos[1], 'Call-ID'), h(srv.pedidos[0], 'Call-ID'));
      expect(h(srv.pedidos[0], 'CSeq'), '1 REGISTER');
      expect(h(srv.pedidos[1], 'CSeq'), '2 REGISTER');
    });

    test('palavra-passe errada: 403 com mensagem, e a palavra-passe não aparece nela', () async {
      srv = await _Servidor.iniciar('segredo');
      final r = await registo.registar(_conta(srv, palavraPasse: 'errada'));
      expect(r.ok, isFalse);
      expect(r.codigo, 403);
      expect(r.mensagem, contains('credenciais inválidas'));
      expect(r.mensagem, isNot(contains('errada')));
    });

    test('funciona com um servidor que não usa qop', () async {
      srv = await _Servidor.iniciar('segredo', qop: false);
      expect((await registo.registar(_conta(srv))).ok, isTrue);
    });

    test('servidor mudo: falha por tempo, não fica pendurado', () async {
      srv = await _Servidor.iniciar('segredo', ignorar: true);
      final r = await registo.registar(_conta(srv));
      expect(r.ok, isFalse);
      expect(r.mensagem, contains('não respondeu'));
      expect(srv.pedidos, isEmpty);
    });

    test('TLS ainda não é suportado: diz-o em vez de fingir', () async {
      srv = await _Servidor.iniciar('segredo');
      final r = await registo.registar(_conta(srv, t: TransporteSip.tls));
      expect(r.ok, isFalse);
      expect(r.mensagem, contains('TLS'));
      expect(srv.pedidos, isEmpty);
    });

    test('desregistar envia Expires: 0', () async {
      srv = await _Servidor.iniciar('segredo');
      await registo.desregistar(_conta(srv));
      expect(srv.pedidos.last, contains('Expires: 0'));
    });
  });
}
