package org.babytrack.app

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.util.Log
import android.widget.RemoteViews
import uniffi.babytrack_core_ffi.NativeLocalStore

/** A glance at saved timer state; tapping opens the tracker for any action. */
class SleepTimerWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, appWidgetIds: IntArray) {
        val pending = goAsync()
        Thread({
            try {
                val database = context.filesDir.resolve("families.db").absolutePath
                NativeLocalStore.open(database).use { local ->
                    ShareCoordinator(context, database).use { sharing ->
                        update(context, runningSleeps(
                            local, sharing, local.families(), sharing.recipientFamilies(),
                        ).count)
                    }
                }
            } catch (failure: Exception) {
                Log.w("BabytrackWidget", "Could not read saved timer state", failure)
            } finally {
                pending.finish()
            }
        }, "babytrack-widget-refresh").start()
    }

    companion object {
        internal fun views(context: Context, activeCount: Int): RemoteViews {
            val status = when (activeCount) {
                0 -> context.getString(R.string.sleep_widget_none)
                1 -> context.getString(R.string.sleep_widget_one)
                else -> context.getString(R.string.sleep_widget_many, activeCount)
            }
            val openTracker = PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
                },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            return RemoteViews(context.packageName, R.layout.sleep_timer_widget).apply {
                setTextViewText(R.id.widget_status, status)
                setOnClickPendingIntent(R.id.widget_root, openTracker)
            }
        }

        internal fun update(context: Context, activeCount: Int) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, SleepTimerWidget::class.java))
            if (ids.isNotEmpty()) manager.updateAppWidget(ids, views(context, activeCount))
        }
    }
}
