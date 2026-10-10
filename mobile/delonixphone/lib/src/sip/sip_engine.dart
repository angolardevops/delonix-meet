/// Estado de registo visto do motor.
enum EstadoRegistoMotor {
  nenhum,
  aRegistar,
  registado,
  apagado,
  falhou,
  aRenovar,
}

/// Estado de uma chamada, com os nomes do liblinphone (ADR-0022) reduzidos ao que a app usa.
enum EstadoChamadaMotor {
  outra,
  aLigar,
  aTocar,
  ligada,
  emCurso,
  terminada,
  erro,
}

sealed class EventoMotor {
  const EventoMotor();
}

class EventoRegisto extends EventoMotor {
  const EventoRegisto(this.estado, this.mensagem);
  final EstadoRegistoMotor estado;
  final String mensagem;
}

class EventoChamada extends EventoMotor {
  const EventoChamada(
    this.estado,
    this.estadoBruto,
    this.mensagem,
    this.motivo,
  );
  final EstadoChamadaMotor estado;

  /// O nome do estado como o motor o dá (para diagnóstico).
  final String estadoBruto;
  final String mensagem;
  final String motivo;
}

/// Medidas de um instante da chamada e do registo, para a prova do spike e para o ecrã de
/// diagnóstico (RF-64). Nunca leva credenciais nem o número marcado.
class DiagnosticoMotor {
  const DiagnosticoMotor(this.valores);
  final Map<String, Object?> valores;

  String? get transporte => valores['transporte'] as String?;
  String? get mediaEncriptacao => valores['mediaEncriptacao'] as String?;
  String? get codec => valores['codec'] as String?;
  double? get rxKbps => (valores['rxKbps'] as num?)?.toDouble();
  int? get msAteRegistar => (valores['msAteRegistar'] as num?)?.toInt();

  @override
  String toString() => valores.toString();
}

/// Porta do motor SIP (ADR-0022). A UI e o controlador só conhecem isto: o liblinphone (ou, no
/// plano B, `flutter_webrtc` com `sip_ua`) vive atrás dela.
abstract interface class SipEngine {
  /// Carrega a configuração `lpconfig` que o servidor entrega por QR e começa a registar.
  /// [raizPem]: raiz de confiança extra, só de laboratório.
  Future<void> iniciar({required String configuracaoXml, String? raizPem});

  Stream<EventoMotor> get eventos;

  /// Marca [destino] (um número de ramal) e devolve se o motor aceitou o pedido.
  Future<bool> ligar(String destino);

  /// Atende a chamada a tocar (se houver).
  Future<bool> atender();

  /// Recusa a chamada a tocar (se houver).
  Future<bool> recusar();
  Future<bool> enviarDtmf(String digitos);
  Future<bool> terminarChamada();
  Future<DiagnosticoMotor> diagnostico();
  Future<void> parar();
}
