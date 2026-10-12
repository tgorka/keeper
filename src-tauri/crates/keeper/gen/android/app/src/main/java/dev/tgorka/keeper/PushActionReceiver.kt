package dev.tgorka.keeper

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * *Approve once* or *Deny* on one of keeper's notifications (Epic 98, story 98.4).
 * Not exported: only the intents `PushNotifier` built reach it, after Android has
 * had the person unlock the device. The action goes to keeper's Rust half
 * (`push_android.rs`), which decides exactly what the notification showed and
 * removes it once sent, or replaces it with "finish in keeper". When keeper is not
 * running, nothing is decided: the notification is replaced with "finish in
 * keeper", which opens it.
 */
class PushActionReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val tag = intent.getStringExtra(PushNotifier.EXTRA_TAG) ?: return
    val event = PushBridge.event("action")
    event.put("tag", tag)
    event.put("category", intent.getStringExtra(PushNotifier.EXTRA_CATEGORY) ?: "")
    event.put("action", intent.getStringExtra(PushNotifier.EXTRA_ACTION) ?: "")
    event.put("userInfo", intent.getStringExtra(PushNotifier.EXTRA_USER_INFO) ?: "{}")
    if (!PushBridge.send(event)) {
      PushNotifier.postFinish(context, tag)
    }
  }
}
