import 'package:flutter/services.dart';

import 'estado_chamada_celular.dart';

class MonitorChamadaCelularAndroid implements MonitorChamadaCelular {
  static const _estados = EventChannel(
    'ao.ngolacloud.delonixphone/estado_chamada_celular',
  );
  static const _permissoes = MethodChannel(
    'ao.ngolacloud.delonixphone/permissoes',
  );

  @override
  Future<bool> permissaoConcedida() async =>
      await _permissoes.invokeMethod<bool>('telefoneConcedida') ?? false;

  @override
  Future<bool> pedirPermissao() async =>
      await _permissoes.invokeMethod<bool>('pedirTelefone') ?? false;

  @override
  Stream<EstadoChamadaCelular> estados() => _estados
      .receiveBroadcastStream()
      .map((nome) => EstadoChamadaCelular.doNome(nome as String))
      .handleError(
        (Object _) => throw const SemPermissaoTelefone(),
        test: (e) => e is PlatformException && e.code == 'sem_permissao',
      );
}
