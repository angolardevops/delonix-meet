import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'app.dart';
import 'src/acordar/intencoes.dart';
import 'src/acordar/orquestrador.dart';
import 'src/chamadas/servico_chamadas.dart';
import 'src/conta/controlador_conta.dart';
import 'src/meet/armazem_meet.dart';
import 'src/sip/linphone_engine_android.dart';
import 'src/telefonia/monitor_chamada_celular_android.dart';

/// Raiz de confiança extra, só de laboratório (debug): `--dart-define=LAB_CA_B64=<base64 do PEM>`.
const _raizLabB64 = String.fromEnvironment('LAB_CA_B64');

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final raiz = _raizLabB64.isEmpty || kReleaseMode
      ? null
      : base64Decode(_raizLabB64);
  final controlador = ControladorConta.padrao();
  final servico = ServicoChamadas(
    LinphoneEngineAndroid(),
    raizPem: raiz == null ? null : utf8.decode(raiz),
  );
  final orquestrador = Orquestrador(
    controlador: controlador,
    servico: servico,
    armazem: ArmazemMeetSeguro(),
    raizConfiavel: raiz,
  );
  runApp(
    DelonixPhoneApp(
      monitor: MonitorChamadaCelularAndroid(),
      controlador: controlador,
      servico: servico,
    ),
  );
  // Depois de a UI estar de pé: o motor arranca com a conta guardada e trata o intent que acordou a app.
  await orquestrador.iniciar(
    iniciais: await Intencoes.iniciais(),
    novas: Intencoes.novas(),
  );
}
