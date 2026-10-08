import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:xml/xml.dart';

import 'conta_sip.dart';

/// O QR, o endereço colado ou o ficheiro falharam de uma forma que o utilizador entende.
class ProvisionamentoInvalido implements Exception {
  const ProvisionamentoInvalido(this.mensagem);
  final String mensagem;
  @override
  String toString() => mensagem;
}

/// O caminho público do resgate (`server/src/extension_provisioning.rs`).
const prefixoResgate = '/api/public/extension-provisioning/';

final _bilhete = RegExp(r'^[0-9a-f]{64}$');

/// Valida o texto lido de um QR ou colado: tem de ser `https://…/api/public/extension-provisioning/<64 hex>`.
/// A configuração leva a palavra-passe SIP, por isso nunca se descarrega por HTTP em claro, e o
/// texto é só um URL: nada do que vem no QR é executado nem seguido para além deste pedido.
Uri enderecoDeProvisionamento(String lido) {
  final uri = Uri.tryParse(lido.trim());
  if (uri == null ||
      uri.scheme != 'https' ||
      uri.host.isEmpty ||
      uri.hasQuery ||
      uri.hasFragment ||
      uri.userInfo.isNotEmpty) {
    throw const ProvisionamentoInvalido(
      'O QR não é um endereço https de provisionamento do Delonix Meet.',
    );
  }
  final i = uri.path.indexOf(prefixoResgate);
  if (i != 0 || !_bilhete.hasMatch(uri.path.substring(prefixoResgate.length))) {
    throw const ProvisionamentoInvalido(
      'O QR não é um bilhete de provisionamento do Delonix Meet.',
    );
  }
  return uri;
}

/// Lê a configuração `lpconfig` que o servidor devolve (a mesma que o Linphone descarrega):
/// `auth_info_0` (utilizador, palavra-passe, domínio) e `proxy_0` (servidor e nome).
ContaSip contaDeLpconfig(String xml) {
  final XmlDocument doc;
  try {
    doc = XmlDocument.parse(xml);
  } on XmlException {
    throw const ProvisionamentoInvalido(
      'A configuração recebida não é XML válido.',
    );
  }
  String? entrada(String seccao, String nome) {
    for (final s in doc.findAllElements('section')) {
      if (s.getAttribute('name') != seccao) continue;
      for (final e in s.findElements('entry')) {
        if (e.getAttribute('name') == nome) return e.innerText.trim();
      }
    }
    return null;
  }

  String exige(String? v, String o) => (v == null || v.isEmpty)
      ? throw ProvisionamentoInvalido('A configuração não tem $o.')
      : v;

  final utilizador = exige(entrada('auth_info_0', 'username'), 'o utilizador');
  final palavraPasse = exige(
    entrada('auth_info_0', 'passwd'),
    'a palavra-passe',
  );
  final dominio = exige(
    entrada('auth_info_0', 'domain') ?? entrada('proxy_0', 'realm'),
    'o domínio',
  );
  final proxy = exige(entrada('proxy_0', 'reg_proxy'), 'o servidor');
  final ServidorSip servidor;
  try {
    servidor = ServidorSip.doUri(proxy);
  } on FormatException {
    throw const ProvisionamentoInvalido(
      'A configuração tem um endereço de servidor inválido.',
    );
  }
  // `reg_identity` é `"Nome" <sip:utilizador@dominio>`; o nome pode faltar.
  final nome = RegExp(r'^"([^"]*)"')
      .firstMatch(entrada('proxy_0', 'reg_identity') ?? '')
      ?.group(1)
      ?.trim();
  return ContaSip(
    nomeExibicao: (nome == null || nome.isEmpty) ? utilizador : nome,
    utilizador: utilizador,
    palavraPasse: palavraPasse,
    dominio: dominio,
    servidor: servidor,
    srtpObrigatorio: entrada('sip', 'media_encryption_mandatory') != '0',
  );
}

/// Porta: transforma o que se leu (QR ou endereço colado) numa conta.
abstract interface class Provisionador {
  Future<ContaSip> resgatar(String lido);
}

/// Descarrega e converte a configuração. **Gasta o bilhete** e o servidor troca a palavra-passe SIP
/// do ramal: o aparelho que lá estava deixa de registar. Por isso nunca se repete sozinho.
class ClienteProvisionamento implements Provisionador {
  /// [raizConfiavel]: certificado PEM extra, só para laboratório (a raiz do `compose-lan.sh`).
  /// Um build de release ignora-o, sempre.
  ClienteProvisionamento({
    List<int>? raizConfiavel,
    bool release = false,
    this._prazo = const Duration(seconds: 15),
  }) : _raiz = release ? null : raizConfiavel;

  final List<int>? _raiz;
  final Duration _prazo;

  @override
  Future<ContaSip> resgatar(String lido) async {
    final uri = enderecoDeProvisionamento(lido);
    final contexto = SecurityContext(withTrustedRoots: true);
    if (_raiz != null && _raiz.isNotEmpty) {
      contexto.setTrustedCertificatesBytes(_raiz);
    }
    final cliente = HttpClient(context: contexto)..connectionTimeout = _prazo;
    try {
      final resposta = await (await cliente.getUrl(uri).timeout(_prazo))
          .close()
          .timeout(_prazo);
      final corpo = await resposta
          .transform(utf8.decoder)
          .join()
          .timeout(_prazo);
      if (resposta.statusCode == 404) {
        throw const ProvisionamentoInvalido(
          'O QR já foi usado, expirou ou não é válido. Peça outro na consola.',
        );
      }
      if (resposta.statusCode != 200) {
        throw ProvisionamentoInvalido(
          'O servidor recusou o provisionamento (HTTP ${resposta.statusCode}).',
        );
      }
      return contaDeLpconfig(corpo);
    } on HandshakeException {
      throw const ProvisionamentoInvalido(
        'O certificado do servidor não é de confiança.',
      );
    } on SocketException {
      throw const ProvisionamentoInvalido(
        'Não foi possível ligar ao servidor.',
      );
    } on TimeoutException {
      throw const ProvisionamentoInvalido('O servidor não respondeu a tempo.');
    } finally {
      cliente.close(force: true);
    }
  }
}
