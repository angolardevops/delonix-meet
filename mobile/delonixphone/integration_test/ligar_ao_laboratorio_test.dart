import 'package:delonixphone/app.dart';
import 'package:delonixphone/src/conta/controlador_conta.dart';
import 'package:delonixphone/src/telefonia/monitor_chamada_celular_android.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:patrol/patrol.dart';

import 'gatilho_lab.dart';

/// A app contra o laboratório REAL do Meet (compose em modo LAN) a partir do emulador: provisiona
/// pelo endereço do QR ou por parâmetros manuais e regista no FreeSWITCH.
///
/// Pré-requisitos: `make compose-up LAN_IP=<ip>` no laboratório, emulador a correr, e `ambiente/patrol.sh`
/// (que arranca o gatilho no anfitrião e passa a raiz de laboratório por --dart-define).
const _prazoRede = Duration(seconds: 40);

Future<void> _abrir(PatrolIntegrationTester $) async {
  await $.pumpWidgetAndSettle(
    DelonixPhoneApp(
      monitor: MonitorChamadaCelularAndroid(),
      controlador: ControladorConta.padrao(),
    ),
  );
}

Future<void> _irParaConfiguracao(PatrolIntegrationTester $) async {
  await $(const Key('configurar-conta')).waitUntilVisible();
  await $(const Key('configurar-conta')).tap();
}

Future<void> _registarEEsperar(
  PatrolIntegrationTester $,
  String esperado,
) async {
  await $(const Key('registar')).tap();
  await $(find.text(esperado)).waitUntilVisible(timeout: _prazoRede);
}

void main() {
  const config = PatrolTesterConfig(settlePolicy: SettlePolicy.trySettle);

  patrolTest(
    'colar o endereço do QR provisiona o ramal e regista no FreeSWITCH',
    config: config,
    ($) async {
      final url = await GatilhoLab.bilhete();
      await _abrir($);
      expect(find.text('Sem conta configurada'), findsOneWidget);

      await _irParaConfiguracao($);
      await $(const Key('campo-endereco')).enterText(url);
      await $(const Key('usar-endereco')).tap();

      await $(const Key('conta-nome')).waitUntilVisible(timeout: _prazoRede);
      expect(find.text('Conta pronta, por registar'), findsOneWidget);
      expect(
        find.byKey(const Key('conta-aviso-sem-cifra')),
        findsOneWidget,
        reason: 'o laboratório só tem UDP: a app tem de o dizer',
      );
      await _registarEEsperar($, 'Registado');
    },
  );

  patrolTest(
    'o mesmo QR usado duas vezes é recusado pelo servidor',
    config: config,
    ($) async {
      final usado = await GatilhoLab.bilheteUsado();
      await _abrir($);
      await _irParaConfiguracao($);
      await $(const Key('campo-endereco')).enterText(usado);
      await $(const Key('usar-endereco')).tap();

      await $(const Key('erro-configuracao'))
          .waitUntilVisible(timeout: _prazoRede);
      expect(find.textContaining('já foi usado'), findsOneWidget);
      expect(find.byKey(const Key('conta-nome')), findsNothing);
    },
  );

  patrolTest(
    'um QR que não é do Meet é recusado sem tocar na rede',
    config: config,
    ($) async {
      await _abrir($);
      await _irParaConfiguracao($);
      await $(const Key('campo-endereco')).enterText(
        'http://10.3.31.15:8443/api/public/extension-provisioning/${'ab' * 32}',
      );
      await $(const Key('usar-endereco')).tap();
      await $(const Key('erro-configuracao')).waitUntilVisible();
      expect(find.textContaining('https'), findsOneWidget);
    },
  );

  Future<void> manual(
    PatrolIntegrationTester $,
    Map<String, dynamic> c, {
    String? palavraPasse,
  }) async {
    await _abrir($);
    await _irParaConfiguracao($);
    await $('Parâmetros manuais').tap();
    await $(const Key('m-nome')).enterText('Emulador');
    await $(const Key('m-utilizador')).enterText(c['utilizador'] as String);
    await $(const Key('m-palavra-passe'))
        .enterText(palavraPasse ?? c['palavraPasse'] as String);
    await $(const Key('m-dominio')).enterText(c['dominio'] as String);
    await $(const Key('m-servidor')).enterText(c['servidor'] as String);
    await $(const Key('m-transporte')).scrollTo();
    await $(const Key('m-transporte')).tap();
    await $('UDP').last.tap();
    await $(const Key('m-guardar')).scrollTo();
    await $(const Key('m-guardar')).tap();
    await $(const Key('conta-nome')).waitUntilVisible();
  }

  patrolTest(
    'parâmetros manuais (recurso) registam no FreeSWITCH',
    config: config,
    ($) async {
      await manual($, await GatilhoLab.credenciais());
      expect(find.text('Emulador'), findsOneWidget);
      await _registarEEsperar($, 'Registado');
    },
  );

  patrolTest(
    'palavra-passe errada: o FreeSWITCH recusa e a app diz porquê, sem a repetir',
    config: config,
    ($) async {
      await manual(
        $,
        await GatilhoLab.credenciais(),
        palavraPasse: 'palavra-passe-errada-123',
      );
      await $(const Key('registar')).tap();
      await $(find.textContaining('credenciais inválidas'))
          .waitUntilVisible(timeout: _prazoRede);
      expect(find.textContaining('palavra-passe-errada-123'), findsNothing);
    },
  );

  patrolTest(
    'negar a câmara no «Ler QR» deixa o recurso à vista',
    config: config,
    ($) async {
      await _abrir($);
      await _irParaConfiguracao($);
      await $(const Key('ler-qr')).tap();
      await $.platform.mobile.denyPermission();
      await $(const Key('qr-sem-camara'))
          .waitUntilVisible(timeout: const Duration(seconds: 15));
    },
  );
}
