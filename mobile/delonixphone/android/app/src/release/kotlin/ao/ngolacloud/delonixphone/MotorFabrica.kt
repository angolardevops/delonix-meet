package ao.ngolacloud.delonixphone

import android.content.Context
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/** Release e profile NÃO levam o liblinphone (AGPL, sem contrato: ADR-0022). */
object MotorFabrica {
    fun criar(contexto: Context): MotorSip = object : MotorSip {
        override fun onMethodCall(chamada: MethodCall, resultado: MethodChannel.Result) =
            resultado.error("motor_indisponivel", "Este build não inclui o motor SIP (licença por assinar).", null)

        override fun onListen(argumentos: Any?, eventos: EventChannel.EventSink) {}
        override fun onCancel(argumentos: Any?) {}
    }
}
