import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../push/push_delonix.dart';

/// Um erro do servidor do Meet que a app sabe explicar. O [codigo] é o `code` estável do envelope de
/// erro do servidor (ex. `devices.too_many`); nunca leva credenciais.
class ErroMeet implements Exception {
  const ErroMeet(this.mensagem, {this.codigo, this.estado});
  final String mensagem;
  final String? codigo;
  final int? estado;
  @override
  String toString() => mensagem;
}

class SessaoMeet {
  const SessaoMeet({required this.accessToken, required this.userId});
  final String accessToken;
  final String userId;
}

/// A API BFF do Meet que a app usa: entrar com a conta e registar o aparelho para ser acordado por push
/// (ADR-0023). Só `https` (um login leva a palavra-passe), com a raiz de laboratório só fora de release.
class ClienteMeet {
  ClienteMeet({
    required this.base,
    List<int>? raizConfiavel,
    bool release = false,
    this.permitirHttpSoParaTestes = false,
    this._prazo = const Duration(seconds: 15),
  }) : _raiz = release ? null : raizConfiavel {
    if (base.scheme != 'https' && !permitirHttpSoParaTestes) {
      throw const ErroMeet('O servidor do Meet tem de ser https.');
    }
  }

  /// A origem do servidor (`https://host:porta`).
  final Uri base;
  final bool permitirHttpSoParaTestes;
  final List<int>? _raiz;
  final Duration _prazo;

  Future<(int, Object?)> _pedir(
    String metodo,
    String caminho, {
    Object? corpo,
    String? token,
  }) async {
    final ctx = SecurityContext(withTrustedRoots: true);
    if (_raiz != null && _raiz.isNotEmpty) {
      ctx.setTrustedCertificatesBytes(_raiz);
    }
    final cliente = HttpClient(context: ctx)..connectionTimeout = _prazo;
    try {
      final pedido = await cliente
          .openUrl(metodo, base.resolve(caminho))
          .timeout(_prazo);
      pedido.headers.set('content-type', 'application/json');
      if (token != null) pedido.headers.set('authorization', 'Bearer $token');
      if (corpo != null) pedido.add(utf8.encode(jsonEncode(corpo)));
      final resposta = await pedido.close().timeout(_prazo);
      final texto = await resposta
          .transform(utf8.decoder)
          .join()
          .timeout(_prazo);
      Object? json;
      try {
        json = texto.isEmpty ? null : jsonDecode(texto);
      } on FormatException {
        json = null;
      }
      return (resposta.statusCode, json);
    } on HandshakeException {
      throw const ErroMeet(
        'O certificado do servidor do Meet não é de confiança.',
      );
    } on SocketException {
      throw const ErroMeet('Não foi possível ligar ao servidor do Meet.');
    } on TimeoutException {
      throw const ErroMeet('O servidor do Meet não respondeu a tempo.');
    } finally {
      cliente.close(force: true);
    }
  }

  ErroMeet _erro(int estado, Object? json, String omissao) {
    final m = json is Map ? json : const {};
    return ErroMeet(
      (m['error'] as String?) ?? omissao,
      codigo: m['code'] as String?,
      estado: estado,
    );
  }

  /// Entra com email e palavra-passe. Uma conta com segundo factor ainda não é suportada: diz-se, em
  /// vez de falhar com um erro opaco.
  Future<SessaoMeet> entrar(String email, String palavraPasse) async {
    final (estado, json) = await _pedir(
      'POST',
      '/api/auth/login',
      corpo: {'email': email, 'password': palavraPasse},
    );
    if (estado == 200 && json is Map) {
      if (json['mfa_token'] != null) {
        throw const ErroMeet(
          'Esta conta tem segundo factor (MFA), que a app ainda não suporta.',
          codigo: 'mfa_necessario',
          estado: 200,
        );
      }
      final token = json['access_token'] as String?;
      final user = (json['user'] as Map?)?['id'] as String?;
      if (token != null && user != null) {
        return SessaoMeet(accessToken: token, userId: user);
      }
    }
    if (estado == 401 || estado == 403) {
      throw const ErroMeet('Email ou palavra-passe incorrectos.', estado: 401);
    }
    throw _erro(estado, json, 'O servidor recusou o login (HTTP $estado).');
  }

  /// O id da organização da pessoa (a primeira, se tiver várias).
  Future<String> organizacao(SessaoMeet s) async {
    final (estado, json) = await _pedir(
      'GET',
      '/api/orgs',
      token: s.accessToken,
    );
    if (estado == 200 && json is List && json.isNotEmpty) {
      return (json.first as Map)['id'] as String;
    }
    throw _erro(estado, json, 'A pessoa não pertence a nenhuma organização.');
  }

  /// Regista (ou renova) o aparelho do ramal da própria pessoa. O servidor guarda o token cifrado e liga o
  /// aparelho a ESTA sessão: terminá-la desliga-o. Devolve se o aparelho foi criado (201) ou renovado (200).
  Future<bool> registarAparelho(
    SessaoMeet s, {
    required String orgId,
    required String aparelhoId,
    required String plataforma,
    required String fornecedor,
    required String tokenPush,
    String versaoApp = '',
  }) async => (await registar(
    s,
    orgId: orgId,
    aparelhoId: aparelhoId,
    plataforma: plataforma,
    fornecedor: fornecedor,
    tokenPush: tokenPush,
    versaoApp: versaoApp,
  )).criado;

  /// Como [registarAparelho], mas devolve também o [GrantPush] que o Meet dá, uma só vez, a um aparelho NOVO
  /// com o fornecedor `delonix`.
  Future<ResultadoRegisto> registar(
    SessaoMeet s, {
    required String orgId,
    required String aparelhoId,
    required String plataforma,
    required String fornecedor,
    required String tokenPush,
    String versaoApp = '',
  }) async {
    final (estado, json) = await _pedir(
      'PUT',
      '/api/orgs/$orgId/my-extension/devices/$aparelhoId',
      token: s.accessToken,
      corpo: {
        'platform': plataforma,
        'provider': fornecedor,
        'push_token': tokenPush,
        'app_version': versaoApp,
      },
    );
    if (estado == 201 || estado == 200) {
      return ResultadoRegisto(
        criado: estado == 201,
        grant: json is Map ? GrantPush.deJson(json['delonix_push']) : null,
      );
    }
    throw _erro(
      estado,
      json,
      'O servidor recusou o registo do aparelho (HTTP $estado).',
    );
  }
}

class ResultadoRegisto {
  const ResultadoRegisto({required this.criado, this.grant});
  final bool criado;
  final GrantPush? grant;
}
