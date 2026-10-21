import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../conta/conta_sip.dart';
import '../sip/lpconfig.dart';
import '../sip/sip_engine.dart';

enum FaseChamada { semChamada, aEntrar, aSair, emCurso }

/// O estado das chamadas da app: liga o motor SIP (ADR-0022) à conta guardada e diz à UI o que se passa.
/// Não sabe nada de liblinphone: fala com a porta [SipEngine].
class ServicoChamadas extends ChangeNotifier {
  ServicoChamadas(this._motor, {this.raizPem});

  final SipEngine _motor;

  /// Raiz de confiança extra, só de laboratório (debug).
  final String? raizPem;

  FaseChamada fase = FaseChamada.semChamada;
  EstadoRegistoMotor registo = EstadoRegistoMotor.nenhum;
  String registoMensagem = '';

  /// `true` se este build não leva o motor (release, sem contrato de licença).
  bool motorIndisponivel = false;

  /// Quem liga, como o motor o dá. Nunca o utilizador SIP de ninguém: é o que a SIP entrega.
  String? erro;

  StreamSubscription<EventoMotor>? _eventos;
  ContaSip? _conta;
  bool _iniciado = false;

  bool get iniciado => _iniciado;

  /// Arranca o motor com a conta (uma vez por conta). Repetir com a mesma conta não faz nada.
  Future<void> iniciar(ContaSip conta) async {
    if (_iniciado && _conta == conta) return;
    await parar();
    erro = null;
    _eventos = _motor.eventos.listen(
      _aoEvento,
      onError: (Object e) {
        erro = 'O motor falhou: $e';
        notifyListeners();
      },
    );
    try {
      await _motor.iniciar(
        configuracaoXml: lpconfigDeConta(conta),
        raizPem: raizPem,
      );
      _conta = conta;
      _iniciado = true;
    } on PlatformException catch (e) {
      await _eventos?.cancel();
      _eventos = null;
      if (e.code == 'motor_indisponivel') {
        motorIndisponivel = true;
      } else {
        erro = 'O motor não arrancou: ${e.message ?? e.code}';
      }
    }
    notifyListeners();
  }

  void _aoEvento(EventoMotor e) {
    switch (e) {
      case EventoRegisto():
        registo = e.estado;
        registoMensagem = e.mensagem;
      case EventoChamada():
        final bruto = e.estadoBruto;
        if (bruto == 'IncomingReceived' || bruto == 'PushIncomingReceived') {
          fase = FaseChamada.aEntrar;
        } else if (e.estado == EstadoChamadaMotor.emCurso) {
          fase = FaseChamada.emCurso;
        } else if (e.estado == EstadoChamadaMotor.terminada ||
            e.estado == EstadoChamadaMotor.erro) {
          fase = FaseChamada.semChamada;
        } else if (e.estado == EstadoChamadaMotor.aLigar ||
            bruto == 'OutgoingRinging') {
          fase = FaseChamada.aSair;
        }
    }
    notifyListeners();
  }

  Future<void> atender() => _motor.atender();
  Future<void> recusar() => _motor.recusar();
  Future<void> terminar() => _motor.terminarChamada();
  Future<bool> ligar(String destino) => _motor.ligar(destino);

  Future<void> parar() async {
    await _eventos?.cancel();
    _eventos = null;
    if (_iniciado) {
      try {
        await _motor.parar();
      } on PlatformException {
        // O motor já não está lá: nada a parar.
      }
    }
    _iniciado = false;
    _conta = null;
    fase = FaseChamada.semChamada;
    registo = EstadoRegistoMotor.nenhum;
  }

  @override
  void dispose() {
    _eventos?.cancel();
    super.dispose();
  }
}
