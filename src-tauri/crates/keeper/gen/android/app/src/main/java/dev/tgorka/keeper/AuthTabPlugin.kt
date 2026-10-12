package dev.tgorka.keeper

import android.app.Activity
import android.net.Uri
import androidx.activity.result.ActivityResult
import androidx.browser.auth.AuthTabIntent
import androidx.browser.customtabs.CustomTabsClient
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class AuthTabArgs {
  lateinit var url: String
  lateinit var scheme: String
}

/**
 * The browser leg of a sign-in on Android (Epic 98, story 98.3; DW-290's recipe):
 * the authorization page in an Auth Tab, Android's twin of iOS's
 * `ASWebAuthenticationSession`.
 *
 * The tab shares its browser's cookies, password manager and passkeys, and when
 * the page redirects to keeper's callback scheme the BROWSER captures the
 * navigation and hands the URL back to this activity as its result. The callback
 * never travels as an intent, so no other app that registered the same scheme can
 * claim it. `web_auth.rs` delivers the answer to the waiting flow.
 *
 * Which browser (R354): the one Android's Custom Tabs pick — the default browser
 * when it supports Custom Tabs (`CustomTabsClient.getPackageName`) — and only if
 * that browser supports Auth Tab. The intent is then sent to that package
 * (`setPackage`), so the tab that opens is the one that was checked, and only
 * then is the tab a real Auth Tab, whose `RESULT_CANCELED` is the person's
 * cancel. Looking further, for another installed browser that has Auth Tab, is
 * DW-1304.
 *
 * Otherwise the intent goes to whatever Android picks for it — a Custom Tab or
 * the browser itself — and the callback arrives through the `keeper://` /
 * `dev.tgorka.keeper:` deep link, as from the default browser. That tab's ending
 * answers `closed`, never `cancelled`: Android reports `RESULT_CANCELED` at once
 * for a browser that takes the intent in its own task, while the person is still
 * signing in, so `closed` ends nothing in Rust.
 *
 * The answer is `{ ending: "callback", url }`, `{ ending: "cancelled" }`,
 * `{ ending: "closed" }` or `{ ending: "failed", reason }`.
 */
@TauriPlugin
class AuthTabPlugin(private val activity: Activity) : Plugin(activity) {
  /** Whether the tab this plugin last launched was a real Auth Tab. */
  private var authTab = false

  /**
   * The package to request the Auth Tab from, or `null` when the browser Android's
   * Custom Tabs pick does not support it. Needs the manifest's `<queries>` entries
   * to see the browsers on Android 11 and later.
   */
  private fun authTabBrowser(): String? {
    val browser = CustomTabsClient.getPackageName(activity, null) ?: return null
    return if (CustomTabsClient.isAuthTabSupported(activity, browser)) browser else null
  }

  @Command
  fun start(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(AuthTabArgs::class.java)
      val browser = authTabBrowser()
      authTab = browser != null
      // What `AuthTabIntent.launch(launcher, url, scheme)` does, through Tauri's own
      // result launcher, which every plugin shares and which was registered before
      // the activity started.
      val intent = AuthTabIntent.Builder().build().intent
      if (browser != null) {
        intent.setPackage(browser)
      }
      intent.data = Uri.parse(args.url)
      intent.putExtra(AuthTabIntent.EXTRA_REDIRECT_SCHEME, args.scheme)
      startActivityForResult(invoke, intent, "authTabResult")
    } catch (e: Exception) {
      // The class names what failed; the message may quote the URL, whose
      // query carries the flow's state, so it is not passed on.
      val answer = JSObject()
      answer.put("ending", "failed")
      answer.put("reason", "the sign-in tab could not open (${e.javaClass.simpleName})")
      invoke.resolve(answer)
    }
  }

  @ActivityCallback
  fun authTabResult(invoke: Invoke, result: ActivityResult) {
    val auth = AuthTabIntent.AuthenticateUserResultContract().parseResult(result.resultCode, result.data)
    val answer = JSObject()
    when (auth.resultCode) {
      AuthTabIntent.RESULT_OK -> {
        val uri = auth.resultUri
        if (uri != null) {
          answer.put("ending", "callback")
          answer.put("url", uri.toString())
        } else {
          answer.put("ending", "failed")
          answer.put("reason", "the sign-in tab finished without the callback address")
        }
      }
      AuthTabIntent.RESULT_CANCELED -> answer.put("ending", if (authTab) "cancelled" else "closed")
      AuthTabIntent.RESULT_VERIFICATION_FAILED, AuthTabIntent.RESULT_VERIFICATION_TIMED_OUT -> {
        answer.put("ending", "failed")
        answer.put("reason", "the browser could not verify keeper's sign-in address")
      }
      else -> {
        answer.put("ending", "failed")
        answer.put("reason", "the sign-in tab ended with an answer keeper does not know")
      }
    }
    invoke.resolve(answer)
  }
}
