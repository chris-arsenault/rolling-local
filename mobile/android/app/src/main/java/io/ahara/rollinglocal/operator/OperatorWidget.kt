package io.ahara.rollinglocal.operator

import android.app.AlarmManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

class OperatorWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        manager.updateAppWidget(ids, render(context))
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action == EXPIRY) update(context)
    }

    companion object {
        const val TOGGLE = "io.ahara.rollinglocal.widget.TOGGLE"
        const val CAMERA = "io.ahara.rollinglocal.widget.CAMERA"
        private const val EXPIRY = "io.ahara.rollinglocal.widget.EXPIRE"

        fun save(context: Context, name: String?, expiresAt: String?) {
            context.getSharedPreferences("widget", Context.MODE_PRIVATE).edit()
                .putString("name", name).putString("expiry", expiresAt).apply()
            update(context)
            val alarm = context.getSystemService(AlarmManager::class.java)
            val refresh = PendingIntent.getBroadcast(context, 3,
                Intent(context, OperatorWidget::class.java).setAction(EXPIRY), PendingIntent.FLAG_IMMUTABLE)
            alarm.cancel(refresh)
            if (expiresAt != null) alarm.set(AlarmManager.RTC, Instant.parse(expiresAt).toEpochMilli(), refresh)
        }

        private fun update(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            manager.updateAppWidget(ComponentName(context, OperatorWidget::class.java), render(context))
        }

        private fun render(context: Context): RemoteViews {
            val prefs = context.getSharedPreferences("widget", Context.MODE_PRIVATE)
            val name = prefs.getString("name", null) ?: "Rolling Local"
            val expiry = prefs.getString("expiry", null)?.let(Instant::parse)?.takeIf { it > Instant.now() }
            val time = expiry?.atZone(ZoneId.systemDefault())?.format(DateTimeFormatter.ofPattern("h:mm a"))
            return RemoteViews(context.packageName, R.layout.operator_widget).apply {
                setTextViewText(R.id.widget_name, name)
                setTextViewText(R.id.widget_status, if (time == null) "Offline" else "Sharing until $time")
                setContentDescription(R.id.widget_toggle, if (time == null) "Start sharing" else "Stop sharing")
                setOnClickPendingIntent(R.id.widget_toggle, open(context, TOGGLE, 1))
                setOnClickPendingIntent(R.id.widget_camera, open(context, CAMERA, 2))
            }
        }

        private fun open(context: Context, action: String, code: Int): PendingIntent {
            val intent = Intent(context, MainActivity::class.java).setAction(action)
                .setFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            return PendingIntent.getActivity(context, code, intent, PendingIntent.FLAG_IMMUTABLE)
        }
    }
}
