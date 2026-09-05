// SPDX-License-Identifier: Apache-2.0
//
// The extension's entry point (Epic 65, Story 65.5, AD-194). A widget
// extension with no widgets: Apple's way to offer a Live Activity is a
// WidgetKit extension whose bundle lists the `ActivityConfiguration`, and
// nothing else is offered here — no Home Screen widget, no timeline, no App
// Group, no push. The bundle's deployment target is iOS 16.2 (project.yml),
// above the app's 16.0, because ActivityKit is 16.1 and the content API
// this extension draws with is 16.2; an older phone simply never loads it.

import SwiftUI
import WidgetKit

@main
struct KeeperIslandBundle: WidgetBundle {
    var body: some Widget {
        KeeperIslandLiveActivity()
    }
}
