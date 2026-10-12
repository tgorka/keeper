package dev.tgorka.keeper

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject
import org.json.JSONObject

/**
 * Where push events go once keeper's Rust half is running (Epic 98, story 98.4;
 * `push_android.rs`).
 *
 * The distributor's broadcasts reach `PushReceiver` and a notification's actions
 * reach `PushActionReceiver` whether or not keeper is running; Android starts the
 * process for them if it has to, and then keeper's Rust half — which starts with
 * `MainActivity` — is not there to ask. So every event is handed to the channel Rust
 * opened at setup when there is one. Without it, a registration event waits here
 * for the next start, and what a person would see is posted from words Rust left
 * last time: keeper's one generic notification for a push, and "finish in keeper"
 * for an action. Nothing here decides what a notification says or offers.
 */
object PushBridge {
  private val lock = Any()
  private var events: Channel? = null
  private val waiting = ArrayList<JSObject>()

  /** Rust's channel, from `UnifiedPushPlugin.start`: send what waited. */
  fun attach(channel: Channel) {
    val queued: List<JSObject>
    synchronized(lock) {
      events = channel
      queued = ArrayList(waiting)
      waiting.clear()
    }
    queued.forEach { channel.send(it) }
  }

  /** Hand `event` to keeper's Rust half; `false` when it is not running. */
  fun send(event: JSObject): Boolean {
    val channel = synchronized(lock) { events } ?: return false
    channel.send(event)
    return true
  }

  /** Hand `event` to keeper now, or when it next starts. */
  fun sendOrKeep(event: JSObject) {
    synchronized(lock) {
      if (events == null) {
        waiting.add(event)
        return
      }
    }
    send(event)
  }

  fun event(kind: String): JSObject {
    val event = JSObject()
    event.put("kind", kind)
    return event
  }
}

/**
 * Posts the notifications Rust composed (`unifiedpush::PhoneNotification`): title,
 * body, its group, its actions — each requiring an unlocked device — and the
 * `user_info` that comes back with an action or a tap. Which actions a device gets
 * is Rust's answer alone (`unifiedpush::phone_notification` offers none below
 * Android 12, the first that can require the unlock); this adds what it was given.
 * Every intent carries a data URI naming its notification and action, so no two
 * notifications ever share a PendingIntent and its extras.
 */
object PushNotifier {
  private const val PREFS = "keeper.push"
  private const val CHANNEL = "channel"
  private const val GENERIC = "generic"
  private const val FINISH = "finish"
  private const val NOTIFICATION_ID = 98
  const val EXTRA_OPEN = "dev.tgorka.keeper.push.OPEN"
  const val EXTRA_TAG = "dev.tgorka.keeper.push.TAG"
  const val EXTRA_CATEGORY = "dev.tgorka.keeper.push.CATEGORY"
  const val EXTRA_ACTION = "dev.tgorka.keeper.push.ACTION"
  const val EXTRA_USER_INFO = "dev.tgorka.keeper.push.USER_INFO"

  private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  /** The channel keeper posts in, and the words it posts while Rust is not running. */
  fun prepare(
    context: Context,
    id: String,
    name: String,
    description: String,
    generic: JSONObject,
    finish: JSONObject,
  ) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val channel = NotificationChannel(id, name, NotificationManager.IMPORTANCE_HIGH)
      channel.description = description
      context.getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }
    prefs(context)
      .edit()
      .putString(CHANNEL, id)
      .putString(GENERIC, generic.toString())
      .putString(FINISH, finish.toString())
      .apply()
  }

  /** keeper's one generic notification: a push keeper's Rust half could not read. */
  fun postGeneric(context: Context) {
    val generic = prefs(context).getString(GENERIC, null) ?: return
    post(context, JSONObject(generic))
  }

  /** Replace the notification `tag` with "finish in keeper", opening keeper. */
  fun postFinish(context: Context, tag: String) {
    val finish = prefs(context).getString(FINISH, null) ?: return
    val notification = JSONObject(finish)
    notification.put("tag", tag)
    post(context, notification)
  }

  fun cancel(context: Context, tag: String) {
    NotificationManagerCompat.from(context).cancel(tag, NOTIFICATION_ID)
  }

  fun post(context: Context, notification: JSONObject) {
    val channel = prefs(context).getString(CHANNEL, null) ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
        PackageManager.PERMISSION_GRANTED
    ) {
      return
    }
    val tag = notification.getString("tag")
    val body = notification.getString("body")
    val userInfo = notification.optJSONObject("user_info")?.toString() ?: "{}"
    val builder = NotificationCompat.Builder(context, channel)
      .setSmallIcon(R.drawable.ic_stat_keeper)
      .setContentTitle(notification.getString("title"))
      .setContentText(body)
      .setStyle(NotificationCompat.BigTextStyle().bigText(body))
      .setGroup(notification.getString("group"))
      .setCategory(NotificationCompat.CATEGORY_MESSAGE)
      .setPriority(NotificationCompat.PRIORITY_HIGH)
      .setAutoCancel(true)
      .setContentIntent(openIntent(context, tag, userInfo))
    if (!notification.isNull("subtitle")) {
      builder.setSubText(notification.getString("subtitle"))
    }
    val category = if (notification.isNull("category")) null else notification.getString("category")
    val actions = notification.optJSONArray("actions")
    if (category != null && actions != null) {
      for (index in 0 until actions.length()) {
        val action = actions.getJSONObject(index)
        val intent = actionIntent(context, tag, category, action.getString("id"), userInfo, index)
        builder.addAction(
          NotificationCompat.Action.Builder(0, action.getString("label"), intent)
            .setAuthenticationRequired(true)
            .build()
        )
      }
    }
    NotificationManagerCompat.from(context).notify(tag, NOTIFICATION_ID, builder.build())
  }

  /**
   * What makes a notification's intent its own: Android tells PendingIntents apart by
   * action, data, type, class and categories — never by extras — so two tags whose
   * request codes collide would otherwise share one, and the later notification's
   * `user_info` would replace the earlier one's.
   */
  private fun identity(tag: String, action: String): Uri =
    Uri.Builder().scheme("keeper-push").authority("notification").appendPath(tag).appendPath(action).build()

  /** A tap opens keeper and hands it the notification's `user_info`. */
  private fun openIntent(context: Context, tag: String, userInfo: String): PendingIntent {
    val intent = Intent(context, MainActivity::class.java)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
      .setData(identity(tag, "open"))
      .putExtra(EXTRA_OPEN, userInfo)
    return PendingIntent.getActivity(
      context,
      tag.hashCode(),
      intent,
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
  }

  /** An action goes to keeper's own receiver, which no other app can reach. */
  private fun actionIntent(
    context: Context,
    tag: String,
    category: String,
    action: String,
    userInfo: String,
    index: Int,
  ): PendingIntent {
    val intent = Intent(context, PushActionReceiver::class.java)
      .setAction("dev.tgorka.keeper.push.$action")
      .setData(identity(tag, action))
      .putExtra(EXTRA_TAG, tag)
      .putExtra(EXTRA_CATEGORY, category)
      .putExtra(EXTRA_ACTION, action)
      .putExtra(EXTRA_USER_INFO, userInfo)
    return PendingIntent.getBroadcast(
      context,
      tag.hashCode() * 31 + index + 1,
      intent,
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
  }
}
