import 'package:flutter/services.dart';

/// Permissões de execução que o motor SIP precisa (o microfone, para falar).
class PermissoesAndroid {
  static const _canal = MethodChannel('ao.ngolacloud.delonixphone/permissoes');

  static Future<bool> microfoneConcedido() async =>
      await _canal.invokeMethod<bool>('microfoneConcedida') ?? false;
  static Future<bool> pedirMicrofone() async =>
      await _canal.invokeMethod<bool>('pedirMicrofone') ?? false;
}
