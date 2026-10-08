package ao.ngolacloud.delonixphone

import android.content.Context
import android.os.Handler
import android.os.Looper
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.linphone.core.Call
import org.linphone.core.Core
import org.linphone.core.CoreListenerStub
import org.linphone.core.Factory
import org.linphone.core.RegistrationState

/**
 * SPIKE da Fase 0 (ADR-0022): o liblinphone atrás da porta `SipEngine`. Só existe em builds de
 * debug (AGPLv3, sem contrato comercial). A configuração é a MESMA que o servidor entrega ao
 * Linphone por QR: o XML `lpconfig` carrega-se tal e qual.
 */
class MotorLinphone(private val contexto: Context) : MotorSip {
    private val principal = Handler(Looper.getMainLooper())
    private var eventos: EventChannel.EventSink? = null
    private var core: Core? = null
    private var chamada: Call? = null
    private var inicioRegisto = 0L
    private var msAteRegistar: Long? = null

    private val ouvinte = object : CoreListenerStub() {
        override fun onAccountRegistrationStateChanged(
            core: Core,
            account: org.linphone.core.Account,
            state: RegistrationState,
            message: String,
        ) {
            if (state == RegistrationState.Ok && msAteRegistar == null) {
                msAteRegistar = System.currentTimeMillis() - inicioRegisto
            }
            emitir(mapOf("tipo" to "registo", "estado" to state.name, "mensagem" to message))
        }

        override fun onCallStateChanged(core: Core, call: Call, state: Call.State, message: String) {
            chamada = if (state == Call.State.End || state == Call.State.Released || state == Call.State.Error) null else call
            emitir(mapOf("tipo" to "chamada", "estado" to state.name, "mensagem" to message, "motivo" to call.reason.name))
        }
    }

    private fun emitir(m: Map<String, Any?>) {
        // Nunca vão credenciais nem o número: só estados.
        principal.post { eventos?.success(m) }
    }

    override fun onMethodCall(c: MethodCall, r: MethodChannel.Result) {
        try {
            when (c.method) {
                "iniciar" -> {
                    iniciar(c.argument<String>("xml")!!, c.argument<String>("raizPem"))
                    r.success(null)
                }
                "ligar" -> {
                    val destino = c.argument<String>("destino")!!
                    val chamadaNova = core!!.invite(destino)
                    chamada = chamadaNova
                    r.success(chamadaNova != null)
                }
                "atender" -> r.success(chamada?.accept() == 0)
                "enviarDtmf" -> r.success(chamada?.sendDtmfs(c.argument<String>("digitos")!!) == 0)
                "terminarChamada" -> r.success(chamada?.terminate() == 0)
                "diagnostico" -> r.success(diagnostico())
                "parar" -> {
                    parar()
                    r.success(null)
                }
                else -> r.notImplemented()
            }
        } catch (e: Exception) {
            r.error("motor_erro", e.message ?: e.javaClass.simpleName, null)
        }
    }

    private fun iniciar(xml: String, raizPem: String?) {
        parar()
        val config = Factory.instance().createConfig(null)
        check(config.loadFromXmlString(xml) == 0) { "o liblinphone não aceitou a configuração XML" }
        val c = Factory.instance().createCoreWithConfig(config, contexto)
        // Raiz de laboratório (a do compose-lan.sh), só em debug: o liblinphone confere o certificado
        // do servidor; sem esta raiz o TLS tem de falhar.
        if (!raizPem.isNullOrEmpty()) c.setRootCaData(raizPem)
        c.setUserAgent("DelonixPhone", "0.1-spike-liblinphone-${c.version}")
        c.addListener(ouvinte)
        msAteRegistar = null
        inicioRegisto = System.currentTimeMillis()
        c.start()
        core = c
    }

    private fun parar() {
        core?.let {
            it.removeListener(ouvinte)
            it.stop()
        }
        core = null
        chamada = null
    }

    private fun diagnostico(): Map<String, Any?> {
        val c = core ?: return mapOf("iniciado" to false)
        val conta = c.defaultAccount
        val ch = chamada ?: c.currentCall
        val params = ch?.currentParams
        val audio = ch?.audioStats
        return mapOf(
            "iniciado" to true,
            "sdk" to c.version,
            "registo" to conta?.state?.name,
            "transporte" to conta?.params?.transport?.name,
            "msAteRegistar" to msAteRegistar,
            "srtpObrigatorio" to c.isMediaEncryptionMandatory,
            "chamada" to ch?.state?.name,
            "mediaEncriptacao" to params?.mediaEncryption?.name,
            "codec" to params?.usedAudioPayloadType?.let { "${it.mimeType}/${it.clockRate}" },
            "rxKbps" to audio?.downloadBandwidth,
            "txKbps" to audio?.uploadBandwidth,
            "perdaPct" to audio?.localLossRate,
            "rttMs" to audio?.roundTripDelay,
            "jitterMs" to audio?.jitterBufferSizeMs,
        )
    }

    override fun onListen(argumentos: Any?, e: EventChannel.EventSink) {
        eventos = e
    }

    override fun onCancel(argumentos: Any?) {
        eventos = null
    }
}
