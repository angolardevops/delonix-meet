import 'package:delonixphone/app.dart';
import 'package:delonixphone/src/telefonia/monitor_chamada_celular_android.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:patrol/patrol.dart';

import 'gatilho_gsm.dart';

/// RF-25, primeira metade: a app VÊ uma chamada celular a entrar. A segunda metade (pôr a
/// chamada SIP em espera) precisa do motor SIP, que ainda não existe.
///
/// Corre num emulador, com ambiente/gatilho-gsm.py no anfitrião: `ambiente/patrol.sh`.
void main() {
  patrolTest(
    'a app segue uma chamada celular: repouso, a tocar, em curso, repouso',
    config: const PatrolTesterConfig(settlePolicy: SettlePolicy.trySettle),
    ($) async {
      const prazo = Duration(seconds: 20);
      Finder estado(String texto) => find.text('Chamada celular: $texto');

      await $.pumpWidgetAndSettle(
        DelonixPhoneApp(monitor: MonitorChamadaCelularAndroid()),
      );

      // Instalação limpa (clearPackageData): ainda sem a permissão do sistema.
      expect(estado('sem permissão'), findsOneWidget);
      await $(const Key('pedir-permissao-telefone')).tap();
      await $.platform.mobile.grantPermissionWhenInUse();

      await $(estado('repouso')).waitUntilVisible(timeout: prazo);

      await GatilhoGsm.ligar();
      await $(estado('a tocar')).waitUntilVisible(timeout: prazo);

      await GatilhoGsm.atender();
      await $(estado('em curso')).waitUntilVisible(timeout: prazo);

      await GatilhoGsm.cancelar();
      await $(estado('repouso')).waitUntilVisible(timeout: prazo);
    },
  );
}
