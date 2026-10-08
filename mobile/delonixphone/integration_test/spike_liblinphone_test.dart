import 'dart:async';
import 'dart:convert';

import 'package:delonixphone/src/conta/provisionamento.dart';
import 'package:delonixphone/src/sip/linphone_engine_android.dart';
import 'package:delonixphone/src/sip/sip_engine.dart';
import 'package:delonixphone/src/telefonia/permissoes_android.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:patrol/patrol.dart';

import 'gatilho_lab.dart';

/// SPIKE da Fase 0 (ADR-0022): o liblinphone contra o FreeSWITCH do laboratório, a partir do
/// emulador. Mede, não afirma: cada resultado sai numa linha `SPIKE {json}` no registo do teste.
///
/// Pré-requisitos: laboratório em modo LAN com TLS nos ramais (#274) e `ambiente/patrol.sh`.
/// Só corre em builds de debug (o liblinphone é AGPL, sem contrato: nenhum release o leva).
const _raizB64 = String.fromEnvironment('LAB_CA_B64');
const _prazo = Duration(seconds: 30);

void spike(String chave, Object? valor) =>
    print('SPIKE ${jsonEncode({chave: valor})}'); // ignore: avoid_print

/// Espera um evento que satisfaça [serve]; falha com os eventos vistos se não vier a tempo.
Future<T> _esperar<T extends EventoMotor>(
  Stream<EventoMotor> eventos,
  bool Function(T) serve,
  String o,
) {
  final vistos = <String>[];
  return eventos
      .map((e) {
        vistos.add(
          e is EventoRegisto
              ? 'registo:${e.estado.name}(${e.mensagem})'
              : 'chamada:${(e as EventoChamada).estadoBruto}',
        );
        return e;
      })
      .where((e) => e is T)
      .cast<T>()
      .firstWhere(serve)
      .timeout(
        _prazo,
        onTimeout: () => throw TestFailure(
          'não veio «$o» em ${_prazo.inSeconds} s; vistos: $vistos',
        ),
      );
}

void main() {
  final raiz = _raizB64.isEmpty ? null : utf8.decode(base64Decode(_raizB64));
  final cliente = ClienteProvisionamento(
    raizConfiavel: _raizB64.isEmpty ? null : base64Decode(_raizB64),
  );
  const config = PatrolTesterConfig(settlePolicy: SettlePolicy.trySettle);

  /// Dá o microfone pelo anfitrião (adb pm grant). O diálogo do sistema prova-se no teste da
  /// permissão do telefone; automatizá-lo aqui falhava com a máquina carregada.
  Future<void> darMicrofone(PatrolIntegrationTester $) async {
    await GatilhoLab.concederMicrofone();
    expect(await PermissoesAndroid.microfoneConcedido(), isTrue);
  }

  patrolTest(
    'liblinphone: o XML do QR regista por TLS no FreeSWITCH (certificado conferido)',
    config: config,
    ($) async {
      final xml = await cliente.baixarConfiguracao(await GatilhoLab.bilhete());
      final motor = LinphoneEngineAndroid();
      final eventos = motor.eventos.asBroadcastStream();
      final sub = eventos.listen((_) {});
      addTearDown(() async {
        await motor.parar();
        await sub.cancel();
      });
      await motor
          .iniciar(configuracaoXml: xml, raizPem: raiz)
          .timeout(
            _prazo,
            onTimeout: () => throw TestFailure(
              'o motor não arrancou em ${_prazo.inSeconds} s (bloqueio no lado nativo)',
            ),
          );
      final r = await _esperar<EventoRegisto>(
        eventos,
        (e) =>
            e.estado == EstadoRegistoMotor.registado ||
            e.estado == EstadoRegistoMotor.falhou,
        'registo',
      );
      final d = await motor.diagnostico();
      spike('registo', d.valores);
      expect(
        r.estado,
        EstadoRegistoMotor.registado,
        reason: 'mensagem: ${r.mensagem}',
      );
      expect(
        d.transporte,
        'Tls',
        reason: 'o QR do laboratório manda registar por TLS',
      );
    },
  );

  patrolTest(
    'liblinphone, controlo negativo: sem a raiz de laboratório o TLS NÃO regista',
    config: config,
    ($) async {
      final xml = await cliente.baixarConfiguracao(await GatilhoLab.bilhete());
      final motor = LinphoneEngineAndroid();
      final eventos = motor.eventos.asBroadcastStream();
      final sub = eventos.listen((_) {});
      addTearDown(() async {
        await motor.parar();
        await sub.cancel();
      });
      await motor
          .iniciar(configuracaoXml: xml)
          .timeout(
            _prazo,
            onTimeout: () => throw TestFailure(
              'o motor não arrancou em ${_prazo.inSeconds} s (bloqueio no lado nativo)',
            ),
          ); // SEM raizPem
      final r = await _esperar<EventoRegisto>(
        eventos,
        (e) =>
            e.estado == EstadoRegistoMotor.registado ||
            e.estado == EstadoRegistoMotor.falhou,
        'resultado',
      );
      spike('registo_sem_raiz', {
        'estado': r.estado.name,
        'mensagem': r.mensagem,
      });
      expect(
        r.estado,
        EstadoRegistoMotor.falhou,
        reason: 'um certificado que o aparelho não conhece não pode registar',
      );
    },
  );

  patrolTest(
    'liblinphone: chamada ao número de acesso (8000) com SRTP e a medida do codec',
    config: config,
    ($) async {
      await darMicrofone($);
      final xml = await cliente.baixarConfiguracao(await GatilhoLab.bilhete());
      final motor = LinphoneEngineAndroid();
      final eventos = motor.eventos.asBroadcastStream();
      final sub = eventos.listen((_) {});
      addTearDown(() async {
        await motor.parar();
        await sub.cancel();
      });
      await motor
          .iniciar(configuracaoXml: xml, raizPem: raiz)
          .timeout(
            _prazo,
            onTimeout: () => throw TestFailure(
              'o motor não arrancou em ${_prazo.inSeconds} s (bloqueio no lado nativo)',
            ),
          );
      await _esperar<EventoRegisto>(
        eventos,
        (e) => e.estado == EstadoRegistoMotor.registado,
        'registo',
      );

      final t0 = DateTime.now();
      expect(
        await motor
            .ligar('8000')
            .timeout(
              _prazo,
              onTimeout: () => throw TestFailure('ligar() bloqueou'),
            ),
        isTrue,
      );
      await _esperar<EventoChamada>(
        eventos,
        (e) => e.estado == EstadoChamadaMotor.emCurso,
        'chamada em curso',
      );
      spike('msAteMedia', DateTime.now().difference(t0).inMilliseconds);
      await Future<void>.delayed(
        const Duration(seconds: 6),
      ); // o IVR fala: há áudio a chegar
      final d = await motor.diagnostico();
      spike('chamada', d.valores);
      expect(
        d.mediaEncriptacao,
        'SRTP',
        reason: 'o perfil dos ramais só aceita SDES-SRTP',
      );
      expect(
        d.rxKbps,
        greaterThan(0),
        reason: 'o IVR do dial-in tem de estar a mandar áudio',
      );
      await motor.terminarChamada();
      await _esperar<EventoChamada>(
        eventos,
        (e) => e.estado == EstadoChamadaMotor.terminada,
        'fim da chamada',
      );
    },
  );

  patrolTest(
    'liblinphone: uma chamada a ENTRAR no ramal toca, atende-se e chega com SRTP',
    config: config,
    ($) async {
      await darMicrofone($);
      final xml = await cliente.baixarConfiguracao(await GatilhoLab.bilhete());
      final conta = contaDeLpconfig(xml);
      final motor = LinphoneEngineAndroid();
      final eventos = motor.eventos.asBroadcastStream();
      final vistos = <String>[];
      final sub = eventos.listen(
        (e) => vistos.add(
          e is EventoChamada
              ? '${e.estadoBruto}/${e.motivo}/${e.mensagem}'
              : 'registo',
        ),
      );
      addTearDown(() async {
        await motor.parar();
        await sub.cancel();
      });
      await motor
          .iniciar(configuracaoXml: xml, raizPem: raiz)
          .timeout(
            _prazo,
            onTimeout: () => throw TestFailure(
              'o motor não arrancou em ${_prazo.inSeconds} s (bloqueio no lado nativo)',
            ),
          );
      await _esperar<EventoRegisto>(
        eventos,
        (e) => e.estado == EstadoRegistoMotor.registado,
        'registo',
      );

      final t0 = DateTime.now();
      await GatilhoLab.tocar(conta.utilizador, conta.dominio);
      await _esperar<EventoChamada>(
        eventos,
        (e) => e.estadoBruto == 'IncomingReceived',
        'chamada a entrar',
      );
      spike('msAteTocar', DateTime.now().difference(t0).inMilliseconds);
      expect(await motor.atender(), isTrue);
      await _esperar<EventoChamada>(
        eventos,
        (e) => e.estado == EstadoChamadaMotor.emCurso,
        'chamada em curso',
      );
      await Future<void>.delayed(
        const Duration(seconds: 5),
      ); // o tom de 440 Hz do servidor
      final d = await motor.diagnostico();
      spike('chamada_a_entrar', d.valores);
      spike('eventos_a_entrar', vistos);
      expect(d.mediaEncriptacao, 'SRTP');
      expect(
        d.rxKbps,
        greaterThan(0),
        reason: 'o servidor toca um tom: tem de chegar áudio',
      );
      await motor.terminarChamada();
      await _esperar<EventoChamada>(
        eventos,
        (e) => e.estado == EstadoChamadaMotor.terminada,
        'fim da chamada',
      );
    },
  );
}
