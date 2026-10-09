import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:delonixphone/src/acordar/orquestrador.dart';
import 'package:delonixphone/src/chamadas/servico_chamadas.dart';
import 'package:delonixphone/src/meet/armazem_meet.dart';
import 'package:delonixphone/src/meet/cliente_meet.dart';
import 'package:delonixphone/src/push/push_delonix.dart';
import 'package:delonixphone/src/sip/sip_engine.dart';
import 'package:flutter_test/flutter_test.dart';

import 'conta_falsos.dart';

class _Motor implements SipEngine {
  final iniciadas = <String>[];
  @override
  Stream<EventoMotor> get eventos => const Stream.empty();
  @override
  Future<void> iniciar({
    required String configuracaoXml,
    String? raizPem,
  }) async => iniciadas.add(configuracaoXml);
  @override
  Future<bool> atender() async => true;
  @override
  Future<bool> recusar() async => true;
  @override
  Future<bool> terminarChamada() async => true;
  @override
  Future<bool> ligar(String destino) async => true;
  @override
  Future<bool> enviarDtmf(String digitos) async => true;
  @override
  Future<DiagnosticoMotor> diagnostico() async => const DiagnosticoMotor({});
  @override
  Future<void> parar() async {}
}

class _ArmazemMeetMemoria implements ArmazemMeet {
  DadosMeet? valor;
  @override
  Future<DadosMeet?> ler() async => valor;
  @override
  Future<void> guardar(DadosMeet d) async => valor = d;
  @override
  Future<void> apagar() async => valor = null;
}

/// Um «Meet» de loopback que aceita o login e o registo do aparelho.
class _Meet {
  _Meet(this.servidor, {this.loginOk = true}) {
    servidor.listen((r) async {
      final corpo = await utf8.decoder.bind(r).join();
      pedidos.add('${r.method} ${r.uri.path}');
      Object json = {};
      var estado = 200;
      if (r.uri.path == '/api/auth/login') {
        if (loginOk) {
          json = {
            'access_token': 'tok',
            'user': {'id': 'u'},
          };
        } else {
          estado = 401;
        }
      } else if (r.uri.path == '/api/orgs') {
        json = [
          {'id': 'org-1'},
        ];
      } else if (r.method == 'PUT') {
        estado = 201;
        ultimoCorpo = jsonDecode(corpo) as Map<String, dynamic>;
        ultimoCaminho = r.uri.path;
        if (ultimoCorpo!['provider'] == 'delonix') {
          json = {
            'id': 'x',
            'delonix_push': {
              'url': 'https://push.exemplo.ao',
              'device_id': 'dev-123',
              'device_secret': 'dpd_segredo-do-aparelho',
            },
          };
        }
      }
      r.response
        ..statusCode = estado
        ..headers.contentType = ContentType.json
        ..write(jsonEncode(json));
      await r.response.close();
    });
  }
  final HttpServer servidor;
  final bool loginOk;
  final pedidos = <String>[];
  Map<String, dynamic>? ultimoCorpo;
  String? ultimoCaminho;
  static Future<_Meet> iniciar({bool loginOk = true}) async => _Meet(
    await HttpServer.bind(InternetAddress.loopbackIPv4, 0),
    loginOk: loginOk,
  );
}

const _url =
    'https://10.0.0.5:8443/api/public/extension-provisioning/abababababababababababababababababababababababababababababababab';

class _PushFalso implements PushDelonix {
  final configurados = <GrantPush>[];
  @override
  Future<void> configurar(GrantPush g) async => configurados.add(g);
  @override
  Future<void> parar() async {}
}

({Orquestrador o, _Motor motor, _ArmazemMeetMemoria meet, RegistoFalso r})
_montar(_Meet meet, {bool release = false, PushDelonix? push}) {
  final motor = _Motor();
  final armazem = _ArmazemMeetMemoria();
  final registo = RegistoFalso();
  final controlador = controladorFalso(registo: registo);
  final o = Orquestrador(
    controlador: controlador,
    servico: ServicoChamadas(motor),
    armazem: armazem,
    release: release,
    push: push,
    fabricaCliente: (_) => ClienteMeet(
      base: Uri.parse('http://127.0.0.1:${meet.servidor.port}'),
      permitirHttpSoParaTestes: true,
    ),
  );
  return (o: o, motor: motor, meet: armazem, r: registo);
}

void main() {
  test('configurar: provisiona, entra, regista o aparelho lab, guarda e arranca o motor', () async {
    final meet = await _Meet.iniciar();
    addTearDown(() => meet.servidor.close(force: true));
    final m = _montar(meet);
    await m.o.tratar({
      'dlx_configurar_url': _url,
      'dlx_email': 'ana@x.ao',
      'dlx_senha': 'segredo-da-ana',
    });
    expect(m.o.estado.value, 'pronto');
    expect(
      meet.ultimoCaminho,
      startsWith('/api/orgs/org-1/my-extension/devices/'),
    );
    expect(meet.ultimoCorpo, containsPair('provider', 'lab'));
    expect(meet.ultimoCorpo!['platform'], 'android');
    // O aparelho fica guardado; a palavra-passe da pessoa NUNCA.
    final guardado = await m.meet.ler();
    expect(guardado!.orgId, 'org-1');
    expect(
      guardado.toJson().values.join(' '),
      isNot(contains('segredo-da-ana')),
    );
    expect(m.motor.iniciadas, hasLength(1));
  });

  test('fornecedor delonix: o segredo que o Meet dá vai ao serviço de push e não fica guardado em Dart', () async {
    final meet = await _Meet.iniciar();
    addTearDown(() => meet.servidor.close(force: true));
    final push = _PushFalso();
    final m = _montar(meet, push: push);
    await m.o.tratar({
      'dlx_configurar_url': _url,
      'dlx_email': 'ana@x.ao',
      'dlx_senha': 'p',
      'dlx_push': 'delonix',
    });
    expect(m.o.estado.value, 'pronto');
    expect(meet.ultimoCorpo, containsPair('provider', 'delonix'));
    expect(push.configurados, hasLength(1));
    expect(push.configurados.single.deviceSecret, 'dpd_segredo-do-aparelho');
    expect(push.configurados.single.url, 'https://push.exemplo.ao');
    final guardado = await m.meet.ler();
    expect(
      guardado!.toJson().values.join(' '),
      isNot(contains('dpd_segredo-do-aparelho')),
      reason:
          'o segredo do aparelho só existe no armazenamento nativo do serviço',
    );
  });

  test(
    'fornecedor lab (por omissão): o serviço de push não é tocado',
    () async {
      final meet = await _Meet.iniciar();
      addTearDown(() => meet.servidor.close(force: true));
      final push = _PushFalso();
      final m = _montar(meet, push: push);
      await m.o.tratar({
        'dlx_configurar_url': _url,
        'dlx_email': 'a@x.ao',
        'dlx_senha': 'p',
      });
      expect(push.configurados, isEmpty);
    },
  );

  test(
    'o mesmo aparelho e o mesmo token de push em configurações seguintes',
    () async {
      final meet = await _Meet.iniciar();
      addTearDown(() => meet.servidor.close(force: true));
      final m = _montar(meet);
      final extras = {
        'dlx_configurar_url': _url,
        'dlx_email': 'a@x.ao',
        'dlx_senha': 'p',
      };
      await m.o.tratar(extras);
      final primeiro = await m.meet.ler();
      await m.o.tratar(extras);
      final segundo = await m.meet.ler();
      expect(
        (segundo!.aparelhoId, segundo.tokenPush),
        (primeiro!.aparelhoId, primeiro.tokenPush),
      );
    },
  );

  test(
    'login recusado: diz-se sem a palavra-passe e o motor não arranca',
    () async {
      final meet = await _Meet.iniciar(loginOk: false);
      addTearDown(() => meet.servidor.close(force: true));
      final m = _montar(meet);
      await m.o.tratar({
        'dlx_configurar_url': _url,
        'dlx_email': 'a@x.ao',
        'dlx_senha': 'palavra-secreta',
      });
      expect(m.o.estado.value, startsWith('falhou'));
      expect(m.o.estado.value, isNot(contains('palavra-secreta')));
      expect(m.motor.iniciadas, isEmpty);
      expect(await m.meet.ler(), isNull);
    },
  );

  test(
    'num RELEASE a configuração por intent é ignorada, mesmo com credenciais',
    () async {
      final meet = await _Meet.iniciar();
      addTearDown(() => meet.servidor.close(force: true));
      final m = _montar(meet, release: true);
      await m.o.tratar({
        'dlx_configurar_url': _url,
        'dlx_email': 'a@x.ao',
        'dlx_senha': 'p',
      });
      expect(
        meet.pedidos,
        isEmpty,
        reason: 'um release não pode falar com o Meet por causa de um intent',
      );
      expect(m.motor.iniciadas, isEmpty);
      await expectLater(
        m.o.configurarDeLaboratorio(_url, 'a@x.ao', 'p'),
        throwsStateError,
      );
    },
  );

  test('acordar: arranca o motor com a conta que já está guardada, sem tocar no Meet', () async {
    final meet = await _Meet.iniciar();
    addTearDown(() => meet.servidor.close(force: true));
    final m = _montar(meet);
    await m.o.controlador.definirManual(contaDeTeste);
    await m.o.tratar({'dlx_acordar': 'call-uuid-1'});
    expect(m.motor.iniciadas, hasLength(1));
    expect(meet.pedidos, isEmpty);
  });

  test('acordar sem conta guardada não faz nada', () async {
    final meet = await _Meet.iniciar();
    addTearDown(() => meet.servidor.close(force: true));
    final m = _montar(meet);
    await m.o.tratar({'dlx_acordar': 'call-uuid-1'});
    expect(m.motor.iniciadas, isEmpty);
  });
}
