package ao.ngolacloud.delonixphone

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.telephony.PhoneStateListener
import android.telephony.TelephonyCallback
import android.telephony.TelephonyManager
import io.flutter.plugin.common.EventChannel

/**
 * Estado da chamada CELULAR (GSM/VoLTE) do sistema, para a app pôr a chamada SIP em espera
 * quando entra uma chamada normal (RF-25). Só o estado: nunca o número (RNF-27), e nenhuma
 * API pública dá o áudio de uma chamada celular.
 */
class EstadoChamadaCelular(private val contexto: Context) : EventChannel.StreamHandler {
    private val telefonia = contexto.getSystemService(TelephonyManager::class.java)
    private var registo: Any? = null

    override fun onListen(argumentos: Any?, eventos: EventChannel.EventSink) {
        if (contexto.checkSelfPermission(Manifest.permission.READ_PHONE_STATE) != PackageManager.PERMISSION_GRANTED) {
            eventos.error("sem_permissao", "READ_PHONE_STATE por conceder", null)
            return
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val ouvinte = object : TelephonyCallback(), TelephonyCallback.CallStateListener {
                override fun onCallStateChanged(estado: Int) = eventos.success(nome(estado))
            }
            telefonia.registerTelephonyCallback(contexto.mainExecutor, ouvinte)
            registo = ouvinte
        } else {
            @Suppress("DEPRECATION")
            val ouvinte = object : PhoneStateListener() {
                @Deprecated("API antiga, necessária abaixo do Android 12")
                override fun onCallStateChanged(estado: Int, numero: String?) = eventos.success(nome(estado))
            }
            @Suppress("DEPRECATION")
            telefonia.listen(ouvinte, PhoneStateListener.LISTEN_CALL_STATE)
            registo = ouvinte
        }
    }

    override fun onCancel(argumentos: Any?) {
        when (val r = registo) {
            is TelephonyCallback -> telefonia.unregisterTelephonyCallback(r)
            is PhoneStateListener -> @Suppress("DEPRECATION") telefonia.listen(r, PhoneStateListener.LISTEN_NONE)
        }
        registo = null
    }

    private fun nome(estado: Int) = when (estado) {
        TelephonyManager.CALL_STATE_RINGING -> "a_tocar"
        TelephonyManager.CALL_STATE_OFFHOOK -> "em_curso"
        else -> "repouso"
    }

    companion object {
        const val CANAL_ESTADOS = "ao.ngolacloud.delonixphone/estado_chamada_celular"
    }
}
