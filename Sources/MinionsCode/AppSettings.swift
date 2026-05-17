import Foundation
import AppKit

@MainActor
@Observable
final class AppSettings {
    static let shared = AppSettings()

    private let defaults = UserDefaults.standard
    private let fontSizeKey = "minionscode.fontSize"
    private let themeKey = "minionscode.theme"
    private let groupByDirKey = "minionscode.groupByDirectory"
    private let notificationsKey = "minionscode.notifications"
    private let soundKey = "minionscode.sound"

    var fontSize: CGFloat {
        didSet { defaults.set(Double(fontSize), forKey: fontSizeKey) }
    }

    var theme: Theme {
        didSet { defaults.set(theme.rawValue, forKey: themeKey) }
    }

    var groupByDirectory: Bool {
        didSet { defaults.set(groupByDirectory, forKey: groupByDirKey) }
    }

    var notificationsEnabled: Bool {
        didSet { defaults.set(notificationsEnabled, forKey: notificationsKey) }
    }

    var soundEnabled: Bool {
        didSet { defaults.set(soundEnabled, forKey: soundKey) }
    }

    init() {
        self.fontSize = defaults.object(forKey: fontSizeKey) as? CGFloat ?? 13
        self.theme = Theme(rawValue: defaults.string(forKey: themeKey) ?? "minion") ?? .minion
        self.groupByDirectory = defaults.object(forKey: groupByDirKey) as? Bool ?? true
        self.notificationsEnabled = defaults.object(forKey: notificationsKey) as? Bool ?? true
        self.soundEnabled = defaults.object(forKey: soundKey) as? Bool ?? true
    }
}

enum Theme: String, CaseIterable {
    case minion = "minion"
    case midnight = "midnight"
    case lava = "lava"

    var displayName: String {
        switch self {
        case .minion: return "Minion (Black/Gold)"
        case .midnight: return "Midnight"
        case .lava: return "Lava"
        }
    }

    var primary: NSColor {
        switch self {
        case .minion: return NSColor(red: 1.0, green: 0.78, blue: 0.10, alpha: 1)
        case .midnight: return NSColor(red: 0.40, green: 0.80, blue: 1.0, alpha: 1)
        case .lava: return NSColor(red: 1.0, green: 0.40, blue: 0.20, alpha: 1)
        }
    }

    var background: NSColor {
        switch self {
        case .minion: return NSColor(red: 0.05, green: 0.05, blue: 0.06, alpha: 1)
        case .midnight: return NSColor(red: 0.04, green: 0.05, blue: 0.10, alpha: 1)
        case .lava: return NSColor(red: 0.07, green: 0.04, blue: 0.05, alpha: 1)
        }
    }

    var foreground: NSColor {
        switch self {
        case .minion: return NSColor(red: 0.93, green: 0.92, blue: 0.85, alpha: 1)
        case .midnight: return NSColor(red: 0.85, green: 0.90, blue: 0.95, alpha: 1)
        case .lava: return NSColor(red: 0.95, green: 0.88, blue: 0.82, alpha: 1)
        }
    }
}
