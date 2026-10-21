import 'dart:math';

import 'package:flutter/foundation.dart';

import '../chamadas/servico_chamadas.dart';
import '../conta/controlador_conta.dart';
import '../conta/provisionamento.dart';
import '../meet/armazem_meet.dart';
import '../meet/cliente_meet.dart';
import '../push/push_delonix.dart';

/// Põe a app a andar quando é acordada (ADR-0023): carrega a conta, arranca o motor SIP, e trata os
/// intents `dlx_*`.
///
/// Um push (de laboratório, por agora) chega como um intent com `dlx_acordar`: a app abre, o motor arranca
/// com a conta guardada, regista-se, e o INVITE que o FreeSWITCH já tem à espera chega-lhe. A configuração
/// por intent (`dlx_configurar_url`, `dlx_email`, `dlx_senha`) é uma comodidade de laboratório e **só corre
/// em debug**: num release é ignorada, e as credenciais nunca se guardam.
class Orquestrador {
  Orquestrador({
    required this.controlador,
    required this.servico,
    required this.armazem,
    ClienteMeet Function(Uri base)? fabricaCliente,
    this.raizConfiavel,
    this.release = kReleaseMode,
    this.versaoApp = '0.1-lab',
    this.push,
  }) : _fabricaCliente =
           fabricaCliente ??
           ((base) => ClienteMeet(
             base: base,
             raizConfiavel: raizConfiavel,
             release: release,
           ));

  final ControladorConta controlador;
  final ServicoChamadas servico;
  final ArmazemMeet armazem;
  final List<int>? raizConfiavel;
  final bool release;
  final String versaoApp;

  /// A ligação própria ao delonix-push (só existe no Android).
  final PushDelonix? push;
  final ClienteMeet Function(Uri base) _fabricaCliente;
  final Random _r = Random.secure();

  /// O que a configuração de laboratório fez, para diagnóstico. Nunca leva credenciais.
  final estado = ValueNotifier<String?>(null);

  Future<void> iniciar({
    Map<String, String> iniciais = const {},
    Stream<Map<String, String>>? novas,
  }) async {
    await controlador.carregar();
    await _garantirMotor();
    await tratar(iniciais);
    novas?.listen(tratar);
  }

  Future<void> _garantirMotor() async {
    final c = controlador.conta;
    if (c != null) await servico.iniciar(c);
  }

  Future<void> tratar(Map<String, String> extras) async {
    if (extras.isEmpty) return;
    final url = extras['dlx_configurar_url'];
    if (url != null && !release) {
      await configurarDeLaboratorio(
        url,
        extras['dlx_email'],
        extras['dlx_senha'],
        fornecedor: extras['dlx_push'] == 'delonix' ? 'delonix' : 'lab',
      );
    }
    if (extras.containsKey('dlx_acordar')) await _garantirMotor();
  }

  /// Provisiona pelo QR, entra no Meet, regista o aparelho (`lab` ou `delonix`) e arranca o motor. Só em debug.
  Future<void> configurarDeLaboratorio(
    String url,
    String? email,
    String? senha, {
    String fornecedor = 'lab',
  }) async {
    if (release) {
      throw StateError('a configuração de laboratório só existe em debug');
    }
    try {
      estado.value = 'a provisionar';
      await controlador.provisionar(url);
      final conta = controlador.conta!;
      if (email != null && senha != null) {
        estado.value = 'a entrar no Meet';
        final u = Uri.parse(url);
        final base = Uri(
          scheme: 'https',
          host: u.host,
          port: u.hasPort ? u.port : null,
        );
        final cliente = _fabricaCliente(base);
        final sessao = await cliente.entrar(email, senha);
        final org = await cliente.organizacao(sessao);
        final anterior = await armazem.ler();
        final aparelho = anterior?.aparelhoId ?? _uuid();
        final tokenPush = anterior?.tokenPush ?? 'lab-${_hex(16)}';
        estado.value = 'a registar o aparelho';
        final r = await cliente.registar(
          sessao,
          orgId: org,
          aparelhoId: aparelho,
          plataforma: 'android',
          fornecedor: fornecedor,
          tokenPush: tokenPush,
          versaoApp: versaoApp,
        );
        // Aparelho `delonix` novo: o Meet deu-lhe o serviço e o segredo, que vão direitos ao serviço nativo.
        if (r.grant != null) await push?.configurar(r.grant!);
        await armazem.guardar(
          DadosMeet(
            base: base.toString(),
            accessToken: sessao.accessToken,
            orgId: org,
            aparelhoId: aparelho,
            tokenPush: tokenPush,
          ),
        );
      }
      await servico.iniciar(conta);
      estado.value = 'pronto';
    } on ErroMeet catch (e) {
      estado.value = 'falhou: ${e.mensagem}';
    } on ProvisionamentoInvalido catch (e) {
      estado.value = 'falhou: ${e.mensagem}';
    }
  }

  String _hex(int bytes) => List.generate(
    bytes,
    (_) => _r.nextInt(256).toRadixString(16).padLeft(2, '0'),
  ).join();

  /// UUID v4 (o servidor escolhe o `device_id` na app: é idempotência do PUT).
  String _uuid() {
    final b = List.generate(16, (_) => _r.nextInt(256));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    String h(int i, int j) =>
        b.sublist(i, j).map((x) => x.toRadixString(16).padLeft(2, '0')).join();
    return '${h(0, 4)}-${h(4, 6)}-${h(6, 8)}-${h(8, 10)}-${h(10, 16)}';
  }
}
