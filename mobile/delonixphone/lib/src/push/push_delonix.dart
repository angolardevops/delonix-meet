import 'package:flutter/services.dart';

/// O que o Meet devolve, uma só vez, ao registar um aparelho com o fornecedor `delonix`: onde está o serviço
/// delonix-push e a credencial deste aparelho. O segredo vai direito ao lado nativo; a app não o guarda em Dart.
class GrantPush {
  const GrantPush({
    required this.url,
    required this.deviceId,
    required this.deviceSecret,
  });
  final String url;
  final String deviceId;
  final String deviceSecret;

  static GrantPush? deJson(Object? json) {
    if (json is! Map) return null;
    final url = json['url'];
    final id = json['device_id'];
    final segredo = json['device_secret'];
    if (url is String && id is String && segredo is String) {
      return GrantPush(url: url, deviceId: id, deviceSecret: segredo);
    }
    return null;
  }
}

/// A ligação própria ao delonix-push, que vive num serviço em primeiro plano do Android.
abstract class PushDelonix {
  Future<void> configurar(GrantPush g);
  Future<void> parar();
}

class PushDelonixCanal implements PushDelonix {
  static const _canal = MethodChannel('ao.ngolacloud.delonixphone/push');

  @override
  Future<void> configurar(GrantPush g) => _canal.invokeMethod('configurar', {
    'url': g.url,
    'segredo': g.deviceSecret,
  });

  @override
  Future<void> parar() => _canal.invokeMethod('parar');
}
