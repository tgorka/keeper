package dev.tgorka.keeper

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.unifiedpush.android.connector.UnifiedPush

@InvokeArg
class PushStartArgs {
  lateinit var events: Channel
  lateinit var channelId: String
  lateinit var channelName: String
  lateinit var channelDescription: String
}

@InvokeArg
class PushRegisterArgs {
  lateinit var distributor: String
}

@InvokeArg
class PushCancelArgs {
  lateinit var tag: String
}

/**
 * Push through the person's UnifiedPush distributor (Epic 98, story 98.4; AD-414):
 * the Kotlin half of `push_android.rs`. It reports which distributors are
 * installed, by package and by the name Android shows for each, registers with the
 * one Rust chooses — only ever one the person confirmed in Settings
 * (`unifiedpush::choose_distributor`) — posts and removes the
 * notifications Rust composes, and passes a tap on one of them back as the
 * notification's `user_info`. The distributor's answers and the actions arrive
 * through `PushReceiver` and `PushActionReceiver`, over the same channel.
 */
@TauriPlugin
class UnifiedPushPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun start(invoke: Invoke) {
    val args = invoke.parseArgs(PushStartArgs::class.java)
    val raw = invoke.getArgs()
    PushNotifier.prepare(
      activity,
      args.channelId,
      args.channelName,
      args.channelDescription,
      raw.getJSONObject("generic"),
      raw.getJSONObject("finish"),
    )
    PushBridge.attach(args.events)
    // A tap that started keeper: `load` sees it only when the webview existed when
    // this plugin was registered, so it is looked for here too (once: `opened`
    // removes the extra).
    opened(activity.intent)
    val answer = JSObject()
    answer.put("sdkInt", Build.VERSION.SDK_INT)
    answer.put("distributors", distributors())
    invoke.resolve(answer)
  }

  /** Every installed distributor: its package and its name on this device. */
  private fun distributors(): JSArray {
    val packages = activity.packageManager
    val installed = JSArray()
    for (name in UnifiedPush.getDistributors(activity)) {
      val label =
        try {
          packages.getApplicationLabel(packages.getApplicationInfo(name, 0)).toString()
        } catch (missing: PackageManager.NameNotFoundException) {
          name
        }
      val one = JSObject()
      one.put("package", name)
      one.put("label", label)
      installed.put(one)
    }
    return installed
  }

  @Command
  fun register(invoke: Invoke) {
    val args = invoke.parseArgs(PushRegisterArgs::class.java)
    UnifiedPush.saveDistributor(activity, args.distributor)
    UnifiedPush.registerApp(activity)
    invoke.resolve()
  }

  @Command
  fun notify(invoke: Invoke) {
    PushNotifier.post(activity, invoke.getArgs())
    invoke.resolve()
  }

  @Command
  fun cancel(invoke: Invoke) {
    val args = invoke.parseArgs(PushCancelArgs::class.java)
    PushNotifier.cancel(activity, args.tag)
    invoke.resolve()
  }

  /** A tap on a notification that started keeper (also looked for in `start`). */
  override fun load(webView: WebView) {
    opened(activity.intent)
  }

  /** A tap on a notification while keeper runs. */
  override fun onNewIntent(intent: Intent) {
    opened(intent)
  }

  private fun opened(intent: Intent?) {
    if (intent == null) return
    val userInfo = intent.getStringExtra(PushNotifier.EXTRA_OPEN) ?: return
    intent.removeExtra(PushNotifier.EXTRA_OPEN)
    val event = PushBridge.event("open")
    event.put("userInfo", userInfo)
    PushBridge.sendOrKeep(event)
  }
}
