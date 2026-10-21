import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../sip/registo_sip.dart';
import 'armazem_conta.dart';
import 'conta_sip.dart';
import 'provisionamento.dart';

/// Raiz de confiança extra, só para laboratório: `--dart-define=LAB_CA_B64=<base64 do PEM>`.
/// Um build de release ignora-a (ver [ClienteProvisionamento]).
const _raizLabB64 = String.fromEnvironment('LAB_CA_B64');

enum FaseRegisto { semConta, parado, aRegistar, registado, falhou }

/// O estado da conta SIP da app: de onde veio, e se o servidor a aceita.
class ControladorConta extends ChangeNotifier {
  ControladorConta({
    required this._armazem,
    required this._provisionador,
    required this._registo,
    bool? permitirSemCifra,
  }) : permitirSemCifra = permitirSemCifra ?? !kReleaseMode;

  factory ControladorConta.padrao() {
    final raiz = _raizLabB64.isEmpty ? null : base64Decode(_raizLabB64);
    return ControladorConta(
      armazem: ArmazemContaSeguro(),
      provisionador: ClienteProvisionamento(
        raizConfiavel: raiz,
        release: kReleaseMode,
      ),
      registo: RegistoSip(raizConfiavel: raiz, release: kReleaseMode),
    );
  }

  final ArmazemConta _armazem;
  final Provisionador _provisionador;
  final ServicoRegisto _registo;

  /// Sem TLS as chaves do SRTP viajam em claro (ADR-0009): só em debug/laboratório (RNF-20).
  final bool permitirSemCifra;

  ContaSip? conta;
  FaseRegisto fase = FaseRegisto.semConta;
  String? mensagem;

  Future<void> carregar() async {
    conta = await _armazem.ler();
    fase = conta == null ? FaseRegisto.semConta : FaseRegisto.parado;
    mensagem = null;
    notifyListeners();
  }

  /// QR lido ou endereço colado. **Gasta o bilhete** e o servidor troca a palavra-passe do ramal.
  /// Lança [ProvisionamentoInvalido] com uma mensagem que o utilizador entende.
  Future<void> provisionar(String lido) async =>
      _definir(await _provisionador.resgatar(lido));

  /// Parâmetros à mão (o caminho de recurso do QR).
  Future<void> definirManual(ContaSip nova) => _definir(nova);

  Future<void> _definir(ContaSip nova) async {
    if (!permitirSemCifra && !nova.servidor.transporte.cifrado) {
      throw const ProvisionamentoInvalido(
        'Transporte sem cifra recusado: use TLS.',
      );
    }
    await _armazem.guardar(nova);
    conta = nova;
    fase = FaseRegisto.parado;
    mensagem = null;
    notifyListeners();
  }

  Future<void> registar() async {
    final c = conta;
    if (c == null || fase == FaseRegisto.aRegistar) return;
    fase = FaseRegisto.aRegistar;
    mensagem = null;
    notifyListeners();
    final r = await _registo.registar(c);
    fase = r.ok ? FaseRegisto.registado : FaseRegisto.falhou;
    mensagem = r.mensagem;
    notifyListeners();
  }

  /// Sair: desregista (melhor esforço) e apaga a conta do aparelho.
  Future<void> remover() async {
    final c = conta;
    if (c != null && fase == FaseRegisto.registado) {
      try {
        await _registo.desregistar(c);
      } on Object {
        // O aparelho vai apagar a conta de qualquer forma; o registo expira no servidor.
      }
    }
    await _armazem.apagar();
    conta = null;
    fase = FaseRegisto.semConta;
    mensagem = null;
    notifyListeners();
  }
}
