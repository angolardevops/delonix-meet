/// Estado da chamada celular (GSM/VoLTE) do sistema. A app nunca vê o áudio nem o número:
/// só isto, para pôr a chamada SIP em espera quando entra uma chamada normal (RF-25).
enum EstadoChamadaCelular {
  repouso,
  aTocar,
  emCurso;

  /// Traduz o nome que vem do código nativo. Um valor desconhecido é um erro, não «repouso»:
  /// esconder um estado novo faria a app ignorar uma chamada a entrar.
  static EstadoChamadaCelular doNome(String nome) => switch (nome) {
    'repouso' => repouso,
    'a_tocar' => aTocar,
    'em_curso' => emCurso,
    _ => throw FormatException('estado de chamada celular desconhecido', nome),
  };
}

/// Falta uma permissão para observar a chamada celular.
class SemPermissaoTelefone implements Exception {
  const SemPermissaoTelefone();
  @override
  String toString() => 'SemPermissaoTelefone';
}

/// Porta: a UI nunca importa o canal nativo, só isto (como o resto da casa, o motor SIP virá
/// atrás de uma porta igual).
abstract interface class MonitorChamadaCelular {
  Future<bool> permissaoConcedida();
  Future<bool> pedirPermissao();

  /// Emite o estado actual ao subscrever e cada mudança. Falha com [SemPermissaoTelefone]
  /// se a permissão não foi dada.
  Stream<EstadoChamadaCelular> estados();
}
