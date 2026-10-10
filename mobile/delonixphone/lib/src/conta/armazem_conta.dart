import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'conta_sip.dart';

/// Onde a conta vive entre arranques. A implementação real usa o Keystore (RNF-23).
abstract interface class ArmazemConta {
  Future<ContaSip?> ler();
  Future<void> guardar(ContaSip conta);
  Future<void> apagar();
}

class ArmazemContaSeguro implements ArmazemConta {
  ArmazemContaSeguro([FlutterSecureStorage? armazem])
    : _a = armazem ?? const FlutterSecureStorage();
  static const _chave = 'conta_sip';
  final FlutterSecureStorage _a;

  @override
  Future<ContaSip?> ler() async {
    final texto = await _a.read(key: _chave);
    if (texto == null) return null;
    try {
      return ContaSip.fromJson(jsonDecode(texto) as Map<String, dynamic>);
    } on Object {
      // Um registo estragado não pode impedir a app de arrancar: trata-se como «sem conta».
      await _a.delete(key: _chave);
      return null;
    }
  }

  @override
  Future<void> guardar(ContaSip conta) =>
      _a.write(key: _chave, value: jsonEncode(conta.toJson()));

  @override
  Future<void> apagar() => _a.delete(key: _chave);
}
