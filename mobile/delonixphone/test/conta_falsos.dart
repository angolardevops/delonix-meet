import 'package:delonixphone/src/conta/armazem_conta.dart';
import 'package:delonixphone/src/conta/conta_sip.dart';
import 'package:delonixphone/src/conta/controlador_conta.dart';
import 'package:delonixphone/src/conta/provisionamento.dart';
import 'package:delonixphone/src/sip/registo_sip.dart';

const contaDeTeste = ContaSip(
  nomeExibicao: 'Ana',
  utilizador: 'ramal_x',
  palavraPasse: 'segredo',
  dominio: 'org.ramais.delonix.meet',
  servidor: ServidorSip(
    anfitriao: '10.0.2.2',
    porta: 5070,
    transporte: TransporteSip.udp,
  ),
);

class ArmazemMemoria implements ArmazemConta {
  ContaSip? valor;
  @override
  Future<ContaSip?> ler() async => valor;
  @override
  Future<void> guardar(ContaSip c) async => valor = c;
  @override
  Future<void> apagar() async => valor = null;
}

class ProvisionadorFalso implements Provisionador {
  ProvisionadorFalso({this.conta, this.erro});
  final ContaSip? conta;
  final ProvisionamentoInvalido? erro;
  final lidos = <String>[];
  @override
  Future<ContaSip> resgatar(String lido) async {
    lidos.add(lido);
    if (erro != null) throw erro!;
    return conta!;
  }
}

class RegistoFalso implements ServicoRegisto {
  RegistoFalso([this.resultado = const ResultadoRegisto.registado()]);
  ResultadoRegisto resultado;
  final chamadas = <String>[];
  @override
  Future<ResultadoRegisto> registar(
    ContaSip c, {
    int expiraSegundos = 3600,
  }) async {
    chamadas.add('registar');
    return resultado;
  }

  @override
  Future<void> desregistar(ContaSip c) async => chamadas.add('desregistar');
}

ControladorConta controladorFalso({
  ArmazemMemoria? armazem,
  ProvisionadorFalso? provisionador,
  RegistoFalso? registo,
  bool? permitirSemCifra,
}) => ControladorConta(
  armazem: armazem ?? ArmazemMemoria(),
  provisionador: provisionador ?? ProvisionadorFalso(conta: contaDeTeste),
  registo: registo ?? RegistoFalso(),
  permitirSemCifra: permitirSemCifra ?? true,
);
