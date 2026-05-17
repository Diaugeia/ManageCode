import SwiftUI

@main
struct MinionsCodeApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 1000, minHeight: 650)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1280, height: 780)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Session") {
                    NotificationCenter.default.post(name: .newSession, object: nil)
                }
                .keyboardShortcut("n")
            }
        }
    }
}

extension Notification.Name {
    static let newSession = Notification.Name("newSession")
}
