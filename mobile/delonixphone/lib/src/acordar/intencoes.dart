import 'package:flutter/services.dart';

/// Os extras `dlx_*` dos intents que abrem a app (ADR-0023): é por aqui que um «push» de laboratório a
/// acorda e que a configuração de laboratório (só em debug) lhe chega. São só texto.
class Intencoes {
  static const _metodos = MethodChannel('ao.ngolacloud.delonixphone/intencao');
  static const _eventos = EventChannel(
    'ao.ngolacloud.delonixphone/intencao_eventos',
  );

  /// O que o intent que arrancou a app trouxe (e nunca mais: o lado nativo limpa-o ao entregar).
  static Future<Map<String, String>> iniciais() async =>
      Map<String, String>.from(
        await _metodos.invokeMethod<Map>('extras') ?? const {},
      );

  /// Os de intents seguintes, com a app já a correr.
  static Stream<Map<String, String>> novas() => _eventos
      .receiveBroadcastStream()
      .map((e) => Map<String, String>.from(e as Map));
}
