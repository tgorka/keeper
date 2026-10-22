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
// what to show.
//
// One notification per item (R1066K-02). iOS names a remote notification
// itself and the person's gateway sends no APNs collapse id, so the item's
// identity is a key on the notification, `keeper_hq_key`, which Rust
// validates and hands back before anything is read (`keeper_nse::answer_key`):
// every notification of the item carries it — composed, generic, and the
// fallback iOS shows when the extension runs out of time — and nothing else
// of keeper's when it is generic. Every answer names the notifications of the
// same item to remove first (`replaces`), including ones this process handed
// iOS earlier that are not on the lock screen yet (`awaits`), which this
// waits for briefly before removing.

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
  /// (`push::PAYLOAD_ACCOUNT`, `push::APPROVAL_INFO`, `push::SHOWN_INFO`,
  /// `hq::push::ITEM_INFO`, `hq::push::KEY_INFO`, `hq::push::APPROVAL_INFO`).
  private static let keeperKeys = [
    "keeper_account", "keeper_approval", "keeper_shown", "keeper_hq", "keeper_hq_item",
    "keeper_hq_approval", "keeper_hq_key",
  ]

  /// What iOS shows when keeper cannot say more: keeper's own words
  /// (`push::GENERIC_TITLE`, `push::GENERIC_BODY`), no category, no keeper
  /// key — whatever the push carried — but the agents item's key Rust
  /// validated, so a later delivery of the item replaces it.
  private static func generic(_ original: UNNotificationContent, key: String? = nil)
    -> UNNotificationContent
  {
    let content = UNMutableNotificationContent()
    content.title = "keeper"
    content.body = "Open keeper to see what arrived."
    content.sound = original.sound
    var info = original.userInfo
    for key in keeperKeys { info.removeValue(forKey: key) }
    if let key { info["keeper_hq_key"] = key }
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
      // The item's key first, so even the fallback is the item's.
      let key = Self.itemKey(item, identifier: request.identifier)
      lock.lock()
      fallback = Self.generic(request.content, key: key)
      lock.unlock()
      receiveItem(item, key: key, request: request)
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

  /// The key of the agents item `item` names, as Rust validates it, this
  /// delivery recorded as handed to iOS (`keeper_nse::answer_key`); nil for
  /// a push that names no item keeper reads. Answers at once: Rust reads
  /// nothing and waits on no other notification for it.
  private static func itemKey(_ item: [String: Any], identifier: String) -> String? {
    let body: [String: Any] = ["hq_key": item, "identifier": identifier]
    guard
      JSONSerialization.isValidJSONObject(body),
      let data = try? JSONSerialization.data(withJSONObject: body),
      let json = String(data: data, encoding: .utf8),
      let answer = ask(json), answer["kind"] as? String == "key"
    else { return nil }
    return answer["key"] as? String
  }

  /// A push from the agents server: Rust reads the item it names with this
  /// device's pairing and says what shows (`keeper_nse::answer_item`).
  private func receiveItem(_ item: [String: Any], key: String?, request: UNNotificationRequest) {
    guard
      let group = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: "group.dev.tgorka.keeper")
    else {
      deliver(Self.generic(request.content, key: key))
      return
    }
    Self.queue.async {
      let body: [String: Any] = [
        "group": group.path,
        "hq": item,
        "identifier": request.identifier,
        "pairings": Self.pairings(),
        "delivered": Self.delivered(),
      ]
      guard
        JSONSerialization.isValidJSONObject(body),
        let data = try? JSONSerialization.data(withJSONObject: body),
        let json = String(data: data, encoding: .utf8)
      else {
        self.deliver(Self.generic(request.content, key: key))
        return
      }
      let answer = Self.ask(json)
      Self.fold(answer)
      self.deliver(Self.itemContent(answer, key: key, from: request.content))
    }
  }

  /// Remove the notifications of the same item Rust's `answer` names, once
  /// every one it awaits — handed to iOS earlier by this process — is on the
  /// lock screen, or after two seconds. This runs on `queue`, never the main
  /// thread.
  private static func fold(_ answer: [String: Any]?) {
    let replaced = answer?["replaces"] as? [String] ?? []
    guard !replaced.isEmpty else { return }
    let awaited = Set(answer?["awaits"] as? [String] ?? [])
    if !awaited.isEmpty {
      let deadline = Date().addingTimeInterval(2)
      while Date() < deadline {
        let shown = Set(delivered().compactMap { $0["id"] as? String })
        if awaited.isSubset(of: shown) { break }
        Thread.sleep(forTimeInterval: 0.1)
      }
    }
    UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: replaced)
  }

  /// The notifications on the lock screen that show an agents item, each
  /// with the item key keeper set on it: what a later delivery of the item
  /// replaces. Waits briefly on the notification center (this runs on
  /// `queue`, never the main thread).
  private static func delivered() -> [[String: Any]] {
    let done = DispatchSemaphore(value: 0)
    var found: [[String: Any]] = []
    UNUserNotificationCenter.current().getDeliveredNotifications { notifications in
      for notification in notifications {
        guard let key = notification.request.content.userInfo["keeper_hq_key"] as? String
        else { continue }
        found.append(["id": notification.request.identifier, "key": key])
      }
      done.signal()
    }
    _ = done.wait(timeout: .now() + 2)
    return found
  }

  /// What to show for Rust's answer to an agents item: its own content for
  /// `item`, nothing for `drop`, keeper's generic words otherwise. Only
  /// `item` carries keeper keys beyond the item's key, and only the ones Rust
  /// composed.
  private static func itemContent(
    _ answer: [String: Any]?, key: String?, from original: UNNotificationContent
  ) -> UNNotificationContent {
    let key = answer?["key"] as? String ?? key
    switch answer?["kind"] as? String {
    case "item":
      guard let answer, let mutable = original.mutableCopy() as? UNMutableNotificationContent
      else { return generic(original, key: key) }
      for key in keeperKeys { mutable.userInfo.removeValue(forKey: key) }
      mutable.title = answer["title"] as? String ?? ""
      mutable.subtitle = answer["subtitle"] as? String ?? ""
      mutable.body = answer["body"] as? String ?? ""
      mutable.categoryIdentifier = answer["category"] as? String ?? ""
      mutable.threadIdentifier = answer["thread_id"] as? String ?? ""
      mutable.userInfo["keeper_shown"] = true
      if let key { mutable.userInfo["keeper_hq_key"] = key }
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
      return generic(original, key: key)
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
