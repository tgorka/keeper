
## Coordinator resolution (2026-07-11): fix 14.3's badge port via UNUserNotificationCenter

The iOS compile break is a 14.3 defect surfaced by this story; fixing it is IN SCOPE for
the 15.4 re-drive (release hygiene = the gate must be green). Replace the iOS
`Platform::set_badge_count` impl's desktop-only `WebviewWindow::set_badge_count` call with
`UNUserNotificationCenter.setBadgeCount` via objc2-user-notifications, as the second
audited function-level `#[allow(unsafe_code)]` FFI exception per the policy in
docs/project-context.md (inventory updated in docs/constraints-and-limitations.md).
Badge remains best-effort: errors logged, never fatal; no permission prompt is triggered
by badge-setting alone (permission flow stays in 14.3's notification path). Desktop impl
unchanged. Then proceed with 15.4's own scope (required CI gate + release hygiene).
