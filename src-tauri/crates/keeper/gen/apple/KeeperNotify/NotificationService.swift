// keeper's notification extension (Epic 98, story 98.1; AD-412).
//
// A push names a room and an event and nothing else. This reads the account's
// session and store passphrase from the keychain group the app shares
// (`KeeperKeychainGroup` in Info.plist, `$(AppIdentifierPrefix)dev.tgorka.keeper`),
// passes them with the App Group container's root to `keeper_nse_handle`
// (`src-tauri/crates/keeper-nse`, linked as `libkeeper_nse.a`), and shows what
// Rust answers: an approval request in keeper's words, an answer of the
// person's own agent, or nothing. Every decision is Rust's; this file moves
// bytes. Ownership across the C ABI: the request buffer is Swift's and lives
// for the call (`withCString`); the answer is Rust's `CString`, read here and
// released exactly once with `keeper_nse_free`.
//
// Rust opens nothing in the group container before the app's one-time move
// has finished there (`keeper_nse::store_of`); until then, and on any
// failure, the notification says keeper's generic words.
//
// The push is the homeserver's to write (the pusher's `default_payload`, and
// whatever reaches the gateway): its alert, its `aps.category` and every
// keeper key in it are untrusted. So on every path but `show` this hands iOS
// keeper's generic title and body with no category — no actions — and no
// keeper key (`generic`); `show` removes every keeper key before it sets its
// own, and only `show` sets `keeper_shown`, which the app's delegate
// (`KeeperPush.swift`) and `push::action_decision` require
// (`keeper_core::agents::push::SHOWN_INFO`, R343).
//
// A push from the agents server (106.6, AD-432) names an item by ids alone,
// under `keeper_hq`. Then this hands Rust the keychain's `agents-server/…`
// pairings and the keeper notifications already delivered, and Rust reads
// that one item from the server with this device's token and answers `item`:
// what to show, and which earlier notifications of the same item it replaces
// (the same item may arrive twice; it shows once).

import Foundation
import Security
import UserNotifications

final class NotificationService: UNNotificationServiceExtension {
  /// One notification at a time in this process (R339): iOS may call
  /// `didReceive` on several instances at once, and each Rust call opens a
  /// client under the extension's 24 MB ceiling. Rust holds its own lock
  /// too (`keeper_nse::handle_json`).
  private static let queue = DispatchQueue(
    label: "dev.tgorka.keeper.notify", qos: .userInitiated)

  private let lock = NSLock()
  private var handler: ((UNNotificationContent) -> Void)?
  private var fallback: UNNotificationContent?

  /// Every key of keeper's a notification's `userInfo` can carry
  /// (`push::PAYLOAD_ACCOUNT`, `push::APPROVAL_INFO`, `push::SHOWN_INFO`).
  private static let keeperKeys = [
    "keeper_account", "keeper_approval", "keeper_shown", "keeper_hq", "keeper_hq_item",
    "keeper_hq_approval",
  ]

  /// What iOS shows when keeper cannot say more: keeper's own words
  /// (`push::GENERIC_TITLE`, `push::GENERIC_BODY`), no category, no keeper
  /// key — whatever the push carried.
  private static func generic(_ original: UNNotificationContent) -> UNNotificationContent {
    let content = UNMutableNotificationContent()
    content.title = "keeper"
    content.body = "Open keeper to see what arrived."
    content.sound = original.sound
    var info = original.userInfo
    for key in keeperKeys { info.removeValue(forKey: key) }
    content.userInfo = info
    return content
  }

  /// Hand `content` to iOS once; later calls do nothing.
  private func deliver(_ content: UNNotificationContent) {
    lock.lock()
    let handler = self.handler
    self.handler = nil
    lock.unlock()
    handler?(content)
  }

  override func didReceive(
    _ request: UNNotificationRequest,
    withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
  ) {
    lock.lock()
    handler = contentHandler
    fallback = Self.generic(request.content)
    lock.unlock()

    let info = request.content.userInfo
    if let item = info["keeper_hq"] as? [String: Any] {
      receiveItem(item, request: request)
      return
    }
    guard
      let room = info["room_id"] as? String,
      let event = info["event_id"] as? String,
      let account = info["keeper_account"] as? String,
      let group = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: "group.dev.tgorka.keeper"),
      let session = Self.keychain("session/\(account)")
    else {
      deliver(Self.generic(request.content))
      return
    }
    let body: [String: Any] = [
      "group": group.path,
      "account": account,
      "room_id": room,
      "event_id": event,
      "session": session,
      "passphrase": Self.keychain("store_passphrase/\(account)") as Any,
    ]
    guard
      let data = try? JSONSerialization.data(withJSONObject: body),
      let json = String(data: data, encoding: .utf8)
    else {
      deliver(Self.generic(request.content))
      return
    }
    Self.queue.async {
      let answer = Self.ask(json)
      self.deliver(Self.content(answer, account: account, from: request.content))
    }
  }

  /// A push from the agents server: Rust reads the item it names with this
  /// device's pairing and says what shows (`keeper_nse::answer_item`).
  private func receiveItem(_ item: [String: Any], request: UNNotificationRequest) {
    guard
      let group = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: "group.dev.tgorka.keeper")
    else {
      deliver(Self.generic(request.content))
      return
    }
    Self.queue.async {
      let body: [String: Any] = [
        "group": group.path,
        "hq": item,
        "pairings": Self.pairings(),
        "delivered": Self.delivered(),
      ]
      guard
        JSONSerialization.isValidJSONObject(body),
        let data = try? JSONSerialization.data(withJSONObject: body),
        let json = String(data: data, encoding: .utf8)
      else {
        self.deliver(Self.generic(request.content))
        return
      }
      let answer = Self.ask(json)
      if answer?["kind"] as? String == "item", let replaced = answer?["replaces"] as? [String],
        !replaced.isEmpty
      {
        UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: replaced)
      }
      self.deliver(Self.itemContent(answer, from: request.content))
    }
  }

  /// The keeper notifications on the lock screen, each with the agents item
  /// it shows: what a second delivery of an item replaces. Waits briefly on
  /// the notification center (this runs on `queue`, never the main thread).
  private static func delivered() -> [[String: Any]] {
    let done = DispatchSemaphore(value: 0)
    var found: [[String: Any]] = []
    UNUserNotificationCenter.current().getDeliveredNotifications { notifications in
      for notification in notifications {
        let info = notification.request.content.userInfo
        guard info["keeper_shown"] as? Bool == true,
          let item = info["keeper_hq_item"] as? [String: Any]
        else { continue }
        found.append(["id": notification.request.identifier, "item": item])
      }
      done.signal()
    }
    _ = done.wait(timeout: .now() + 2)
    return found
  }

  /// What to show for Rust's answer to an agents item: its own content for
  /// `item`, nothing for `drop`, keeper's generic words otherwise. Only
  /// `item` carries keeper keys, and only the ones Rust composed.
  private static func itemContent(_ answer: [String: Any]?, from original: UNNotificationContent)
    -> UNNotificationContent
  {
    switch answer?["kind"] as? String {
    case "item":
      guard let answer, let mutable = original.mutableCopy() as? UNMutableNotificationContent
      else { return generic(original) }
      for key in keeperKeys { mutable.userInfo.removeValue(forKey: key) }
      mutable.title = answer["title"] as? String ?? ""
      mutable.subtitle = answer["subtitle"] as? String ?? ""
      mutable.body = answer["body"] as? String ?? ""
      mutable.categoryIdentifier = answer["category"] as? String ?? ""
      mutable.threadIdentifier = answer["thread_id"] as? String ?? ""
      mutable.userInfo["keeper_shown"] = true
      if let item = answer["item"] as? [String: Any] {
        mutable.userInfo["keeper_hq_item"] = item
      }
      if let approval = answer["approval"] as? [String: Any] {
        // What the action handler decides: exactly the request and digest
        // this notification showed.
        mutable.userInfo["keeper_hq_approval"] = approval
      }
      return mutable
    case "drop":
      return UNNotificationContent()
    default:
      return generic(original)
    }
  }

  /// Every `agents-server/<base URL>` item of keeper's service in the shared
  /// group, by key: Rust picks the server whose replica of the item's
  /// database this device holds.
  private static func pairings() -> [String: String] {
    guard
      let group = Bundle.main.object(forInfoDictionaryKey: "KeeperKeychainGroup") as? String
    else { return [:] }
    let query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: "dev.tgorka.keeper",
      kSecAttrAccessGroup as String: group,
      kSecReturnAttributes as String: true,
      kSecMatchLimit as String: kSecMatchLimitAll,
    ]
    // Names first, then each value: a query for every item's data at once
    // is not one every keychain answers.
    var items: CFTypeRef?
    guard SecItemCopyMatching(query as CFDictionary, &items) == errSecSuccess,
      let found = items as? [[String: Any]]
    else { return [:] }
    var pairings: [String: String] = [:]
    for item in found {
      guard let key = item[kSecAttrAccount as String] as? String,
        key.hasPrefix("agents-server/"),
        let value = keychain(key)
      else { continue }
      pairings[key] = value
    }
    return pairings
  }

  override func serviceExtensionTimeWillExpire() {
    lock.lock()
    let content = fallback
    lock.unlock()
    if let content { deliver(content) }
  }

  /// Rust's answer to the request `json`, parsed; nil when there is none.
  private static func ask(_ json: String) -> [String: Any]? {
    guard let raw = json.withCString({ keeper_nse_handle($0) }) else { return nil }
    defer { keeper_nse_free(raw) }
    let text = String(cString: raw)
    guard let data = text.data(using: .utf8) else { return nil }
    return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
  }

  /// What to show for Rust's `answer`: its own content for `show`, nothing
  /// for `drop`, keeper's generic words otherwise. Only `show` carries
  /// keeper keys, and only the ones it sets: `account` is the account whose
  /// store answered.
  private static func content(
    _ answer: [String: Any]?, account: String, from original: UNNotificationContent
  ) -> UNNotificationContent {
    switch answer?["kind"] as? String {
    case "show":
      guard let answer, let mutable = original.mutableCopy() as? UNMutableNotificationContent
      else { return generic(original) }
      for key in keeperKeys { mutable.userInfo.removeValue(forKey: key) }
      mutable.title = answer["title"] as? String ?? ""
      mutable.subtitle = answer["subtitle"] as? String ?? ""
      mutable.body = answer["body"] as? String ?? ""
      mutable.categoryIdentifier = answer["category"] as? String ?? ""
      mutable.threadIdentifier = answer["thread_id"] as? String ?? ""
      mutable.userInfo["keeper_account"] = account
      mutable.userInfo["keeper_shown"] = true
      if let approval = answer["approval"] as? [String: Any] {
        // What the action handler decides (rung 4): exactly the record and
        // digest this notification showed.
        mutable.userInfo["keeper_approval"] = approval
      }
      return mutable
    case "drop":
      return UNNotificationContent()
    default:
      return generic(original)
    }
  }

  /// The keychain item `key` of keeper's service in the shared group.
  private static func keychain(_ key: String) -> String? {
    guard
      let group = Bundle.main.object(forInfoDictionaryKey: "KeeperKeychainGroup") as? String
    else { return nil }
    let query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: "dev.tgorka.keeper",
      kSecAttrAccount as String: key,
      kSecAttrAccessGroup as String: group,
      kSecReturnData as String: true,
      kSecMatchLimit as String: kSecMatchLimitOne,
    ]
    var item: CFTypeRef?
    guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
      let data = item as? Data
    else { return nil }
    return String(data: data, encoding: .utf8)
  }
}
