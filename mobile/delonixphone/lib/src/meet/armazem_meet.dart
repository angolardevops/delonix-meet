import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// O que a app guarda do Meet entre arranques: a origem do servidor, o token de acesso e o aparelho. A
/// palavra-passe da pessoa NUNCA se guarda.
class DadosMeet {
  const DadosMeet({
    required this.base,
    required this.accessToken,
    required this.orgId,
    required this.aparelhoId,
    required this.tokenPush,
  });
  final String base;
  final String accessToken;
  final String orgId;
  final String aparelhoId;
  final String tokenPush;

  Map<String, String> toJson() => {
    'base': base,
    'accessToken': accessToken,
    'orgId': orgId,
    'aparelhoId': aparelhoId,
    'tokenPush': tokenPush,
  };

  static DadosMeet fromJson(Map<String, dynamic> j) => DadosMeet(
    base: j['base'] as String,
    accessToken: j['accessToken'] as String,
    orgId: j['orgId'] as String,
    aparelhoId: j['aparelhoId'] as String,
    tokenPush: j['tokenPush'] as String,
  );

  @override
  String toString() => 'DadosMeet($base, org $orgId, aparelho $aparelhoId)';
}

abstract interface class ArmazemMeet {
  Future<DadosMeet?> ler();
  Future<void> guardar(DadosMeet d);
  Future<void> apagar();
}

class ArmazemMeetSeguro implements ArmazemMeet {
  ArmazemMeetSeguro([FlutterSecureStorage? a])
    : _a = a ?? const FlutterSecureStorage();
  static const _chave = 'dados_meet';
  final FlutterSecureStorage _a;

  @override
  Future<DadosMeet?> ler() async {
    final t = await _a.read(key: _chave);
    if (t == null) return null;
    try {
      return DadosMeet.fromJson(jsonDecode(t) as Map<String, dynamic>);
    } on Object {
      await _a.delete(key: _chave);
      return null;
    }
  }

  @override
  Future<void> guardar(DadosMeet d) =>
      _a.write(key: _chave, value: jsonEncode(d.toJson()));

  @override
  Future<void> apagar() => _a.delete(key: _chave);
}
