import 'package:xml/xml.dart';

import '../conta/conta_sip.dart';

/// A configuração `lpconfig` que o motor (liblinphone, ADR-0022) carrega, feita a partir da conta
/// guardada: as mesmas chaves que o servidor entrega por QR (`extension_provisioning.rs`), para o
/// motor arrancar sem repetir o QR (que é de uso único e trocaria a palavra-passe SIP). Constrói-se
/// com um *builder* de XML: nada do que vem da conta pode fechar um elemento.
String lpconfigDeConta(ContaSip c) {
  // O nome vai entre aspas num cabeçalho SIP: sem aspas, barras, ângulos nem controlo.
  final nome = c.nomeExibicao
      .replaceAll(RegExp(r'["\\<>\x00-\x1f]'), '')
      .trim();
  final identidade = nome.isEmpty
      ? '<sip:${c.utilizador}@${c.dominio}>'
      : '"$nome" <sip:${c.utilizador}@${c.dominio}>';
  final proxy = c.servidor.uri;
  final b = XmlBuilder();
  b.processing('xml', 'version="1.0" encoding="UTF-8"');
  b.element(
    'config',
    // `namespaceUri:` (a forma nova) parte este documento em `xml` 7.1 (medido); a antiga declara o
    // espaço de nomes por omissão, que é o que o Linphone lê.
    // ignore: deprecated_member_use
    namespaces: {'http://www.linphone.org/xsds/lpconfig.xsd': null},
    nest: () {
      void seccao(String nome, Map<String, String> entradas) => b.element(
        'section',
        attributes: {'name': nome},
        nest: () {
          entradas.forEach(
            (k, v) => b.element(
              'entry',
              attributes: {'name': k, 'overwrite': 'true'},
              nest: v,
            ),
          );
        },
      );
      seccao('sip', {
        'default_proxy': '0',
        'media_encryption': 'srtp',
        'media_encryption_mandatory': c.srtpObrigatorio ? '1' : '0',
      });
      seccao('auth_info_0', {
        'username': c.utilizador,
        'passwd': c.palavraPasse,
        'realm': c.dominio,
        'domain': c.dominio,
      });
      seccao('proxy_0', {
        'reg_proxy': '<$proxy>',
        'reg_route': '<$proxy;lr>',
        'reg_identity': identidade,
        'realm': c.dominio,
        'reg_expires': '3600',
        'reg_sendregister': '1',
        'publish': '0',
      });
    },
  );
  return b.buildDocument().toXmlString();
}
