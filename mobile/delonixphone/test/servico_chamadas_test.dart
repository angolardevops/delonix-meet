import 'dart:async';

import 'package:delonixphone/src/chamadas/servico_chamadas.dart';
import 'package:delonixphone/src/conta/conta_sip.dart';
import 'package:delonixphone/src/sip/sip_engine.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

class _Motor implements SipEngine {
  final controlo = StreamController<EventoMotor>.broadcast();
  final chamadas = <String>[];
  String? xml;
  PlatformException? falha;

  @override
  Stream<EventoMotor> get eventos => controlo.stream;
  @override
  Future<void> iniciar({
    required String configuracaoXml,
    String? raizPem,
  }) async {
    if (falha != null) throw falha!;
    xml = configuracaoXml;
    chamadas.add('iniciar');
  }

  @override
  Future<bool> atender() async {
    chamadas.add('atender');
    return true;
  }

  @override
  Future<bool> recusar() async {
    chamadas.add('recusar');
    return true;
  }

  @override
  Future<bool> terminarChamada() async {
    chamadas.add('terminar');
    return true;
  }

  @override
  Future<bool> ligar(String destino) async {
    chamadas.add('ligar');
    return true;
  }

  @override
  Future<bool> enviarDtmf(String digitos) async => true;
  @override
  Future<DiagnosticoMotor> diagnostico() async => const DiagnosticoMotor({});
  @override
  Future<void> parar() async => chamadas.add('parar');

  void emite(EventoMotor e) => controlo.add(e);
}

const _conta = ContaSip(
  nomeExibicao: 'Ana',
  utilizador: 'ramal_x',
  palavraPasse: 'segredo',
  dominio: 'org.ramais.delonix.meet',
  servidor: ServidorSip(
    anfitriao: '10.0.0.5',
    porta: 5071,
    transporte: TransporteSip.tls,
  ),
);

EventoChamada _ev(String bruto, EstadoChamadaMotor e) =>
    EventoChamada(e, bruto, '', '');

Future<void> _drena() => Future<void>.delayed(Duration.zero);

void main() {
  test('iniciar carrega o lpconfig da conta; repetir com a mesma conta não reinicia', () async {
    final m = _Motor();
    final s = ServicoChamadas(m);
    await s.iniciar(_conta);
    await s.iniciar(_conta);
    expect(m.chamadas, ['iniciar']);
    expect(m.xml, contains('ramal_x'));
    expect(s.iniciado, isTrue);
  });

  test(
    'a sequência de uma chamada a entrar: toca, atende-se, em curso, termina',
    () async {
      final m = _Motor();
      final s = ServicoChamadas(m);
      await s.iniciar(_conta);
      m.emite(const EventoRegisto(EstadoRegistoMotor.registado, 'Ok'));
      await _drena();
      expect(s.registo, EstadoRegistoMotor.registado);
      m.emite(_ev('IncomingReceived', EstadoChamadaMotor.aTocar));
      await _drena();
      expect(s.fase, FaseChamada.aEntrar);
      await s.atender();
      m.emite(_ev('StreamsRunning', EstadoChamadaMotor.emCurso));
      await _drena();
      expect(s.fase, FaseChamada.emCurso);
      m.emite(_ev('End', EstadoChamadaMotor.terminada));
      await _drena();
      expect(s.fase, FaseChamada.semChamada);
      expect(m.chamadas, containsAllInOrder(['iniciar', 'atender']));
    },
  );

  test('a chamada a SAIR a tocar não é confundida com uma a entrar', () async {
    final m = _Motor();
    final s = ServicoChamadas(m);
    await s.iniciar(_conta);
    m.emite(_ev('OutgoingRinging', EstadoChamadaMotor.aTocar));
    await _drena();
    expect(s.fase, FaseChamada.aSair);
  });

  test('um build sem motor (release) diz-se e não rebenta', () async {
    final m = _Motor()..falha = PlatformException(code: 'motor_indisponivel');
    final s = ServicoChamadas(m);
    await s.iniciar(_conta);
    expect(s.motorIndisponivel, isTrue);
    expect(s.iniciado, isFalse);
    expect(s.erro, isNull);
  });

  test('outro erro do motor fica visível sem a palavra-passe', () async {
    final m = _Motor()
      ..falha = PlatformException(
        code: 'motor_erro',
        message: 'config inválida',
      );
    final s = ServicoChamadas(m);
    await s.iniciar(_conta);
    expect(s.erro, contains('config inválida'));
    expect(s.erro, isNot(contains('segredo')));
  });

  test('recusar e terminar passam ao motor; parar limpa o estado', () async {
    final m = _Motor();
    final s = ServicoChamadas(m);
    await s.iniciar(_conta);
    m.emite(_ev('IncomingReceived', EstadoChamadaMotor.aTocar));
    await _drena();
    await s.recusar();
    await s.terminar();
    await s.parar();
    expect(m.chamadas, containsAllInOrder(['recusar', 'terminar', 'parar']));
    expect((s.fase, s.iniciado), (FaseChamada.semChamada, false));
  });
}
