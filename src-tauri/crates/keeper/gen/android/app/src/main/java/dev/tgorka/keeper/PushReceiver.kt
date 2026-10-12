package dev.tgorka.keeper

import android.content.Context
import org.unifiedpush.android.connector.MessagingReceiver

/**
 * The UnifiedPush connector's receiver (Epic 98, story 98.4): what the person's
 * distributor tells keeper. Every event goes to keeper's Rust half
 * (`push_android.rs`), which decides what it means; none is read here.
 *
 * A push that arrives while keeper is not running — Android stopped it, and the
 * distributor's broadcast started only this receiver — cannot be read: keeper's
 * Rust half, with its accounts and keys, starts with `MainActivity`. It is posted
 * as keeper's one generic notification, in words Rust left at its last start, and
 * each such push replaces the last.
 */
class PushReceiver : MessagingReceiver() {
  override fun onNewEndpoint(context: Context, endpoint: String, instance: String) {
    val event = PushBridge.event("endpoint")
    event.put("endpoint", endpoint)
    PushBridge.sendOrKeep(event)
  }

  override fun onRegistrationFailed(context: Context, instance: String) {
    PushBridge.sendOrKeep(PushBridge.event("failed"))
  }

  override fun onUnregistered(context: Context, instance: String) {
    PushBridge.sendOrKeep(PushBridge.event("unregistered"))
  }

  override fun onMessage(context: Context, message: ByteArray, instance: String) {
    val event = PushBridge.event("message")
    event.put("body", String(message, Charsets.UTF_8))
    if (!PushBridge.send(event)) {
      PushNotifier.postGeneric(context)
    }
  }
}
