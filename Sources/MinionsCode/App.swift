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
                Button("New Shell Tab") {
                    NotificationCenter.default.post(name: .newSession, object: nil)
                }
                .keyboardShortcut("t")
                Button("New Window") {
                    NotificationCenter.default.post(name: .newSession, object: nil)
                }
                .keyboardShortcut("n")
                Divider()
                Button("Close Tab") {
                    NotificationCenter.default.post(name: .closeActiveTab, object: nil)
                }
                .keyboardShortcut("w")
            }
            CommandGroup(after: .toolbar) {
                Button("Zoom In") {
                    NotificationCenter.default.post(name: .zoomIn, object: nil)
                }
                .keyboardShortcut("+")
                Button("Zoom Out") {
                    NotificationCenter.default.post(name: .zoomOut, object: nil)
                }
                .keyboardShortcut("-")
                Button("Reset Zoom") {
                    NotificationCenter.default.post(name: .zoomReset, object: nil)
                }
                .keyboardShortcut("0")
                Divider()
                Button("Toggle Sidebar") {
                    NotificationCenter.default.post(name: .toggleSidebar, object: nil)
                }
                .keyboardShortcut("\\")
            }
            CommandMenu("Tabs") {
                ForEach(1...9, id: \.self) { i in
                    Button("Show Tab \(i)") {
                        NotificationCenter.default.post(name: .selectTab, object: i - 1)
                    }
                    .keyboardShortcut(KeyEquivalent(Character("\(i)")))
                }
                Divider()
                Button("Next Tab") {
                    NotificationCenter.default.post(name: .nextTab, object: nil)
                }
                .keyboardShortcut("]", modifiers: [.command, .shift])
                Button("Previous Tab") {
                    NotificationCenter.default.post(name: .prevTab, object: nil)
                }
                .keyboardShortcut("[", modifiers: [.command, .shift])
            }
        }
    }
}

extension Notification.Name {
    static let newSession = Notification.Name("newSession")
    static let toggleSidebar = Notification.Name("toggleSidebar")
    static let closeActiveTab = Notification.Name("closeActiveTab")
    static let zoomIn = Notification.Name("zoomIn")
    static let zoomOut = Notification.Name("zoomOut")
    static let zoomReset = Notification.Name("zoomReset")
    static let selectTab = Notification.Name("selectTab")
    static let nextTab = Notification.Name("nextTab")
    static let prevTab = Notification.Name("prevTab")
}
