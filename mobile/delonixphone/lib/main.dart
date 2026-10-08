import 'package:flutter/widgets.dart';

import 'app.dart';
import 'src/conta/controlador_conta.dart';
import 'src/telefonia/monitor_chamada_celular_android.dart';

void main() => runApp(
  DelonixPhoneApp(
    monitor: MonitorChamadaCelularAndroid(),
    controlador: ControladorConta.padrao(),
  ),
);
