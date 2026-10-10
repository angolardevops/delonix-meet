package ao.ngolacloud.delonixphone

import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

/**
 * A ponte nativa do motor SIP (porta `SipEngine` do lado Dart, ADR-0022). Quem a implementa vive
 * em `src/debug` (liblinphone, AGPL) ou é o stub de `src/release` e `src/profile`.
 */
interface MotorSip : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {
    companion object {
        const val CANAL = "ao.ngolacloud.delonixphone/motor"
        const val CANAL_EVENTOS = "ao.ngolacloud.delonixphone/motor_eventos"
    }
}
