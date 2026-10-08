import 'package:flutter/services.dart';

import 'sip_engine.dart';

/// Adaptador do liblinphone (Android) para a porta [SipEngine]. SPIKE: só funciona em builds de
/// debug; num release o lado nativo responde `motor_indisponivel` (ADR-0022, licença por assinar).
class LinphoneEngineAndroid implements SipEngine {
  static const _metodos = MethodChannel('ao.ngolacloud.delonixphone/motor');
  static const _canalEventos = EventChannel(
    'ao.ngolacloud.delonixphone/motor_eventos',
  );

  @override
  Stream<EventoMotor> get eventos =>
      _canalEventos.receiveBroadcastStream().map((e) {
        final m = Map<String, Object?>.from(e as Map);
        if (m['tipo'] == 'registo') {
          return EventoRegisto(
            _registo(m['estado'] as String),
            (m['mensagem'] as String?) ?? '',
          );
        }
        final bruto = m['estado'] as String;
        return EventoChamada(
          _chamada(bruto),
          bruto,
          (m['mensagem'] as String?) ?? '',
          (m['motivo'] as String?) ?? '',
        );
      });

  static EstadoRegistoMotor _registo(String n) => switch (n) {
    'None' => EstadoRegistoMotor.nenhum,
    'Progress' => EstadoRegistoMotor.aRegistar,
    'Ok' => EstadoRegistoMotor.registado,
    'Cleared' => EstadoRegistoMotor.apagado,
    'Failed' => EstadoRegistoMotor.falhou,
    'Refreshing' => EstadoRegistoMotor.aRenovar,
    _ => throw FormatException('estado de registo desconhecido', n),
  };

  static EstadoChamadaMotor _chamada(String n) => switch (n) {
    'OutgoingInit' || 'OutgoingProgress' => EstadoChamadaMotor.aLigar,
    'OutgoingRinging' ||
    'OutgoingEarlyMedia' ||
    'IncomingReceived' ||
    'PushIncomingReceived' => EstadoChamadaMotor.aTocar,
    'Connected' => EstadoChamadaMotor.ligada,
    'StreamsRunning' => EstadoChamadaMotor.emCurso,
    'End' || 'Released' => EstadoChamadaMotor.terminada,
    'Error' => EstadoChamadaMotor.erro,
    _ => EstadoChamadaMotor.outra,
  };

  @override
  Future<void> iniciar({required String configuracaoXml, String? raizPem}) =>
      _metodos.invokeMethod<void>('iniciar', {
        'xml': configuracaoXml,
        'raizPem': raizPem,
      });

  @override
  Future<bool> ligar(String destino) async =>
      await _metodos.invokeMethod<bool>('ligar', {'destino': destino}) ?? false;

  @override
  Future<bool> atender() async =>
      await _metodos.invokeMethod<bool>('atender') ?? false;

  @override
  Future<bool> recusar() async =>
      await _metodos.invokeMethod<bool>('recusar') ?? false;

  @override
  Future<bool> enviarDtmf(String digitos) async =>
      await _metodos.invokeMethod<bool>('enviarDtmf', {'digitos': digitos}) ??
      false;

  @override
  Future<bool> terminarChamada() async =>
      await _metodos.invokeMethod<bool>('terminarChamada') ?? false;

  @override
  Future<DiagnosticoMotor> diagnostico() async => DiagnosticoMotor(
    Map<String, Object?>.from(
      await _metodos.invokeMethod<Map>('diagnostico') ?? {},
    ),
  );

  @override
  Future<void> parar() => _metodos.invokeMethod<void>('parar');
}
