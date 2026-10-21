package ao.ngolacloud.delonixphone

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.util.Log
import ao.ngolacloud.push.PushMessage
import ao.ngolacloud.push.android.PushHandler
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * Uma chamada a entrar (`kind = incoming_call`) vira uma notificação de ecrã inteiro que abre a app com os
 * extras `dlx_*` que o `Orquestrador` já conhece. Um serviço em segundo plano NÃO pode abrir uma Activity
 * directamente (Android 10+): a notificação de ecrã inteiro é o caminho que a plataforma dá às chamadas.
 * O payload só leva o `call_uuid` e o número curto de quem liga, nunca credenciais.
 */
object ChamadaPorPush : PushHandler {
    private const val CANAL = "chamadas"
    const val TAG = "DelonixPush"

    override fun onMessage(context: Context, message: PushMessage) {
        val p = message.payload as? JsonObject ?: return
        if (p["kind"]?.jsonPrimitive?.contentOrNull != "incoming_call") return
        val callUuid = p["call_uuid"]?.jsonPrimitive?.contentOrNull.orEmpty().take(64)
        val caller = p["caller"]?.jsonPrimitive?.contentOrNull.orEmpty().take(32)
        Log.i(TAG, "chamada a entrar por push id=${message.id} call=$callUuid caller=$caller")

        val abrir = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra("dlx_acordar", "1")
            putExtra("dlx_call_uuid", callUuid)
            putExtra("dlx_caller", caller)
        }
        val pi = PendingIntent.getActivity(
            context, callUuid.hashCode(), abrir, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CANAL, "Chamadas", NotificationManager.IMPORTANCE_HIGH))
        nm.notify(
            callUuid.hashCode(),
            Notification.Builder(context, CANAL)
                .setSmallIcon(android.R.drawable.stat_sys_phone_call)
                .setContentTitle("Chamada a entrar")
                .setContentText(if (caller.isEmpty()) "Número desconhecido" else caller)
                .setCategory(Notification.CATEGORY_CALL)
                .setFullScreenIntent(pi, true)
                .setContentIntent(pi)
                .setAutoCancel(true)
                .build(),
        )
    }
}
