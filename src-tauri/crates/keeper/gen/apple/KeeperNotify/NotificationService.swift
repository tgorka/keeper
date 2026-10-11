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
// failure, the notification stays as the gateway sent it ("generic").

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
    fallback = request.content
    lock.unlock()

    let info = request.content.userInfo
    guard
      let room = info["room_id"] as? String,
      let event = info["event_id"] as? String,
      let account = info["keeper_account"] as? String,
      let group = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: "group.dev.tgorka.keeper"),
      let session = Self.keychain("session/\(account)")
    else {
      deliver(request.content)
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
      deliver(request.content)
      return
    }
    Self.queue.async {
      let answer = Self.ask(json)
      self.deliver(Self.content(answer, from: request.content))
    }
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
  /// for `drop`, the gateway's notification otherwise.
  private static func content(
    _ answer: [String: Any]?, from original: UNNotificationContent
  ) -> UNNotificationContent {
    switch answer?["kind"] as? String {
    case "show":
      guard let answer, let mutable = original.mutableCopy() as? UNMutableNotificationContent
      else { return original }
      mutable.title = answer["title"] as? String ?? ""
      mutable.subtitle = answer["subtitle"] as? String ?? ""
      mutable.body = answer["body"] as? String ?? ""
      mutable.categoryIdentifier = answer["category"] as? String ?? ""
      mutable.threadIdentifier = answer["thread_id"] as? String ?? ""
      if let approval = answer["approval"] as? [String: Any] {
        // What the action handler decides (rung 4): exactly the record and
        // digest this notification showed.
        mutable.userInfo["keeper_approval"] = approval
      }
      return mutable
    case "drop":
      return UNNotificationContent()
    default:
      return original
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
