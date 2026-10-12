// SPDX-License-Identifier: Apache-2.0
//
// keeper's notification delegate (Epic 98, story 98.1; AD-412, UX-DR144).
//
// `src-tauri/crates/keeper/src/push_ios.rs` calls `keeper_push_install` right
// after Tauri's build, before UIKit starts: iOS hands a notification's
// response to the delegate set before launch finishes, and an action that
// launches keeper in the background runs no Tauri `setup()`. This object
// then becomes `UNUserNotificationCenter`'s delegate and keeps the one that
// was there — the notification plugin's, which drops every push-triggered
// response — for everything that is not keeper's push. It registers the two
// categories Rust describes (`keeper_core::agents::push::registered_categories`)
// with every action needing the phone unlocked, and asks UIKit for remote
// notifications; the token reaches Rust through tao's app delegate.
//
// Nothing is decided here. An action goes to `keeper_push_action` with the
// notification's category, the action and its `userInfo`, and Rust decides
// what — if anything — is sent; a tap goes to `keeper_push_open`, and Rust
// decides which room opens. Threads: an action is handed to Rust on a
// background queue (it blocks up to `push::ACTION_BUDGET`) under a background
// task, and the completion handler is called on the main queue afterwards.
// Ownership: arguments are Swift's buffers for the call (`withCString`); a
// string Rust returns is read here and released once with `keeper_push_free`.

import Foundation
import UIKit
import UserNotifications

final class KeeperPush: NSObject, UNUserNotificationCenterDelegate {
  static let shared = KeeperPush()

  /// The delegate that was there before keeper's: the plugin's.
  private var forward: UNUserNotificationCenterDelegate?
  private var asked = false

  /// Become the delegate (once per object it replaces), register the
  /// categories, and — the first time — ask for remote notifications once
  /// UIKit's main loop runs.
  func install(categories: [[String: Any]]) {
    let center = UNUserNotificationCenter.current()
    if center.delegate !== self {
      forward = center.delegate
      center.delegate = self
    }
    center.setNotificationCategories(Set(categories.compactMap(Self.category)))
    if !asked {
      asked = true
      DispatchQueue.main.async {
        UIApplication.shared.registerForRemoteNotifications()
      }
    }
  }

  /// One category as Rust describes it: `{id, actions: [{id, label, destructive}]}`.
  private static func category(_ spec: [String: Any]) -> UNNotificationCategory? {
    guard let id = spec["id"] as? String, let actions = spec["actions"] as? [[String: Any]]
    else { return nil }
    return UNNotificationCategory(
      identifier: id,
      actions: actions.compactMap { action in
        guard let actionId = action["id"] as? String, let label = action["label"] as? String
        else { return nil }
        var options: UNNotificationActionOptions = [.authenticationRequired]
        if action["destructive"] as? Bool == true { options.insert(.destructive) }
        return UNNotificationAction(identifier: actionId, title: label, options: options)
      },
      intentIdentifiers: [],
      options: [])
  }

  /// A notification keeper composed: the extension's `show`, or one keeper
  /// re-posted. Only those carry `keeper_shown` (`push::SHOWN_INFO`): the
  /// extension removes it, with every other keeper key, from whatever the
  /// gateway sent, so a push that names a keeper account and category but
  /// was not composed on this phone is not keeper's (R343).
  private static func ours(_ notification: UNNotification) -> Bool {
    notification.request.content.userInfo["keeper_shown"] as? Bool == true
  }

  /// `userInfo` as JSON text, string keys only.
  private static func json(_ object: Any) -> String? {
    guard JSONSerialization.isValidJSONObject(object),
      let data = try? JSONSerialization.data(withJSONObject: object)
    else { return nil }
    return String(data: data, encoding: .utf8)
  }

  private static func plain(_ userInfo: [AnyHashable: Any]) -> [String: Any] {
    var plain: [String: Any] = [:]
    for (key, value) in userInfo {
      if let key = key as? String { plain[key] = value }
    }
    return plain
  }

  func userNotificationCenter(
    _ center: UNUserNotificationCenter,
    willPresent notification: UNNotification,
    withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
  ) {
    if Self.ours(notification) {
      // The extension already decided to show it; with keeper in front it
      // shows as a banner too, with its actions.
      completionHandler([.banner, .list, .sound])
      return
    }
    if forward?.userNotificationCenter?(
      center, willPresent: notification, withCompletionHandler: completionHandler) == nil
    {
      completionHandler([])
    }
  }

  func userNotificationCenter(
    _ center: UNUserNotificationCenter,
    didReceive response: UNNotificationResponse,
    withCompletionHandler completionHandler: @escaping () -> Void
  ) {
    guard Self.ours(response.notification) else {
      if forward?.userNotificationCenter?(
        center, didReceive: response, withCompletionHandler: completionHandler) == nil
      {
        completionHandler()
      }
      return
    }
    let content = response.notification.request.content
    let userInfo = Self.plain(content.userInfo)
    switch response.actionIdentifier {
    case UNNotificationDefaultActionIdentifier:
      if let text = Self.json(userInfo) {
        text.withCString { keeper_push_open($0) }
      }
      completionHandler()
    case UNNotificationDismissActionIdentifier:
      completionHandler()
    default:
      let request: [String: Any] = [
        "category": content.categoryIdentifier,
        "action": response.actionIdentifier,
        "user_info": userInfo,
      ]
      guard let text = Self.json(request) else {
        completionHandler()
        return
      }
      var task = UIBackgroundTaskIdentifier.invalid
      task = UIApplication.shared.beginBackgroundTask(withName: "keeper.decision") {
        UIApplication.shared.endBackgroundTask(task)
        task = .invalid
      }
      DispatchQueue.global(qos: .userInitiated).async {
        let answer = Self.act(text)
        if let answer { Self.repost(answer) }
        DispatchQueue.main.async {
          completionHandler()
          if task != .invalid {
            UIApplication.shared.endBackgroundTask(task)
            task = .invalid
          }
        }
      }
    }
  }

  /// Rust's answer to an action: nil when the decision was sent, else the
  /// notification to post.
  private static func act(_ request: String) -> [String: Any]? {
    guard let raw = request.withCString({ keeper_push_action($0) }) else { return nil }
    defer { keeper_push_free(raw) }
    guard let data = String(cString: raw).data(using: .utf8) else { return nil }
    return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
  }

  /// Post the notification Rust composed: the decision is to be finished in
  /// keeper, and tapping it opens the session room.
  private static func repost(_ answer: [String: Any]) {
    let content = UNMutableNotificationContent()
    content.title = answer["title"] as? String ?? ""
    content.body = answer["body"] as? String ?? ""
    content.threadIdentifier = answer["thread_id"] as? String ?? ""
    if let info = answer["user_info"] as? [String: Any] { content.userInfo = info }
    UNUserNotificationCenter.current().add(
      UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
  }
}

/// Become the notification delegate with the categories `categories` (JSON)
/// describes, and ask for remote notifications. Called by Rust on the main
/// thread; safe to call again (`push_ios::set_up`).
@_cdecl("keeper_push_install")
public func keeper_push_install(_ categories: UnsafePointer<CChar>) {
  let text = String(cString: categories)
  let specs =
    (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [[String: Any]] ?? []
  KeeperPush.shared.install(categories: specs)
}
