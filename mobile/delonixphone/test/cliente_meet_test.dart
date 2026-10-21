import 'dart:convert';
import 'dart:io';

import 'package:delonixphone/src/meet/cliente_meet.dart';
import 'package:flutter_test/flutter_test.dart';

/// Um «Meet» mínimo em http de loopback: só o que o cliente usa.
class _Meet {
  _Meet(this.servidor) {
    servidor.listen((r) async {
      final corpo = await utf8.decoder.bind(r).join();
      pedidos.add((
        r.method,
        r.uri.path,
        r.headers.value('authorization'),
        corpo,
      ));
      final (estado, json) = resposta(r.method, r.uri.path, corpo);
      r.response
        ..statusCode = estado
        ..headers.contentType = ContentType.json
        ..write(jsonEncode(json));
      await r.response.close();
    });
  }
  final HttpServer servidor;
  final pedidos = <(String, String, String?, String)>[];
  late (int, Object?) Function(String, String, String) resposta;

  static Future<_Meet> iniciar() async =>
      _Meet(await HttpServer.bind(InternetAddress.loopbackIPv4, 0));
  Uri get base => Uri.parse('http://127.0.0.1:${servidor.port}');
  void fechar() => servidor.close(force: true);
}

ClienteMeet _cliente(_Meet m) =>
    ClienteMeet(base: m.base, permitirHttpSoParaTestes: true);

void main() {
  late _Meet meet;
  setUp(() async => meet = await _Meet.iniciar());
  tearDown(() => meet.fechar());

  test(
    'recusa um servidor que não é https (um login leva a palavra-passe)',
    () {
      expect(
        () => ClienteMeet(base: Uri.parse('http://meet.exemplo.ao')),
        throwsA(isA<ErroMeet>()),
      );
    },
  );

  group('entrar', () {
    test('devolve o token e o utilizador, e manda o corpo certo', () async {
      meet.resposta = (_, _, _) => (
        200,
        {
          'access_token': 'tok',
          'user': {'id': 'u-1'},
        },
      );
      final s = await _cliente(meet).entrar('ana@x.ao', 'segredo');
      expect((s.accessToken, s.userId), ('tok', 'u-1'));
      expect(jsonDecode(meet.pedidos.single.$4), {
        'email': 'ana@x.ao',
        'password': 'segredo',
      });
    });

    test('um segundo factor diz-se, em vez de falhar às cegas', () async {
      meet.resposta = (_, _, _) => (
        200,
        {
          'mfa_token': 't',
          'methods': ['totp'],
        },
      );
      await expectLater(
        _cliente(meet).entrar('a@x.ao', 'p'),
        throwsA(
          isA<ErroMeet>().having((e) => e.codigo, 'codigo', 'mfa_necessario'),
        ),
      );
    });

    test(
      'palavra-passe errada: mensagem clara, sem repetir a palavra-passe',
      () async {
        meet.resposta = (_, _, _) =>
            (401, {'error': 'x', 'code': 'auth.invalid'});
        await expectLater(
          _cliente(meet).entrar('a@x.ao', 'palavra-passe-secreta'),
          throwsA(
            isA<ErroMeet>().having(
              (e) => e.mensagem,
              'mensagem',
              allOf(contains('incorrectos'), isNot(contains('secreta'))),
            ),
          ),
        );
      },
    );
  });

  test(
    'registarAparelho: PUT com o token da sessão; 201 = criado, 200 = renovado',
    () async {
      meet.resposta = (_, _, _) => (201, {'id': 'a'});
      final s = const SessaoMeet(accessToken: 'tok', userId: 'u');
      expect(
        await _cliente(meet).registarAparelho(
          s,
          orgId: 'o',
          aparelhoId: 'a1',
          plataforma: 'android',
          fornecedor: 'lab',
          tokenPush: 'tp',
        ),
        isTrue,
      );
      final (metodo, caminho, auth, corpo) = meet.pedidos.single;
      expect(
        (metodo, caminho, auth),
        ('PUT', '/api/orgs/o/my-extension/devices/a1', 'Bearer tok'),
      );
      expect(jsonDecode(corpo), containsPair('push_token', 'tp'));
      meet.resposta = (_, _, _) => (200, {'id': 'a'});
      expect(
        await _cliente(meet).registarAparelho(
          s,
          orgId: 'o',
          aparelhoId: 'a1',
          plataforma: 'android',
          fornecedor: 'lab',
          tokenPush: 'tp',
        ),
        isFalse,
      );
    },
  );

  test(
    'registarAparelho: o código do servidor chega à UI (aparelho revogado)',
    () async {
      meet.resposta = (_, _, _) =>
          (409, {'error': 'revogado', 'code': 'devices.revoked'});
      await expectLater(
        _cliente(meet).registarAparelho(
          const SessaoMeet(accessToken: 't', userId: 'u'),
          orgId: 'o',
          aparelhoId: 'a',
          plataforma: 'android',
          fornecedor: 'lab',
          tokenPush: 't',
        ),
        throwsA(
          isA<ErroMeet>().having((e) => e.codigo, 'codigo', 'devices.revoked'),
        ),
      );
    },
  );

  test(
    'um servidor que não responde dá uma mensagem, não uma excepção crua',
    () async {
      final cliente = _cliente(meet); // antes de fechar: depois já não há porta
      await meet.servidor.close(force: true);
      await expectLater(
        cliente.entrar('a@x.ao', 'p'),
        throwsA(isA<ErroMeet>()),
      );
    },
  );
}
