import AppKit
import SwiftTerm

enum SessionMode {
    case shell
    case claude(resumeId: String?)
}

@MainActor
final class TerminalSession: @unchecked Sendable {
    let terminalView: LocalProcessTerminalView
    let mode: SessionMode
    let id: String
    let cwd: String
    var isReadOnly: Bool = false
    private(set) var isRunning = false
    nonisolated(unsafe) private var keyMonitor: Any?

    init(mode: SessionMode = .shell, cwd: String? = nil) {
        self.mode = mode
        self.id = {
            if case .claude(let rid) = mode, let rid { return rid }
            return UUID().uuidString
        }()
        self.cwd = cwd ?? FileManager.default.homeDirectoryForCurrentUser.path

        self.terminalView = LocalProcessTerminalView(frame: NSRect(x: 0, y: 0, width: 900, height: 600))
        TerminalSession.applyDefaultTheme(to: terminalView)
        terminalView.optionAsMetaKey = true
        terminalView.disableFullRedrawOnAnyChanges = true

        let env = buildEnv()
        switch mode {
        case .shell:
            let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
            terminalView.startProcess(executable: shell, args: ["-l"], environment: env, execName: "-\(URL(fileURLWithPath: shell).lastPathComponent)", currentDirectory: self.cwd)
        case .claude(let resumeId):
            let claudePath = findClaude()
            var args = [String]()
            if let rid = resumeId { args = ["--resume", rid] }
            terminalView.startProcess(executable: claudePath, args: args, environment: env, execName: "claude", currentDirectory: self.cwd)
        }
        isRunning = true
        installKeyMonitor()
    }

    deinit {
        // keyMonitor is nonisolated-safe to release through MainActor.assumeIsolated guards
        if let m = keyMonitor {
            NSEvent.removeMonitor(m)
        }
    }

    func sendCommand(_ command: String) {
        terminalView.send(txt: "\(command)\n")
    }

    func toggleReadOnly() {
        isReadOnly.toggle()
    }

    func terminate() {
        terminalView.process.terminate()
        isRunning = false
    }

    /// Intercepts key events bound for our terminal view to:
    /// 1. Translate macOS shortcuts (Cmd+Backspace, Cmd+Left, Option+Backspace) to readline sequences.
    /// 2. Swallow input events when in read-only mode.
    /// Returns nil to consume the event, the original event to let it through.
    private func installKeyMonitor() {
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown]) { [weak self] event in
            guard let self = self else { return event }
            // Only intercept when our terminal is the first responder
            guard let window = self.terminalView.window,
                  window.firstResponder === self.terminalView else {
                return event
            }
            return self.handleKey(event)
        }
    }

    private func handleKey(_ event: NSEvent) -> NSEvent? {
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)

        // Read-only mode: drop everything except scrolling/copy/select-all/find
        if isReadOnly {
            if mods.contains(.command) {
                let chars = event.charactersIgnoringModifiers?.lowercased() ?? ""
                if ["c", "a", "f"].contains(chars) { return event }
            }
            if event.specialKey == .pageUp || event.specialKey == .pageDown
                || event.specialKey == .home || event.specialKey == .end
                || event.specialKey == .upArrow || event.specialKey == .downArrow {
                return event
            }
            return nil
        }

        // Cmd+Backspace -> Ctrl+U (kill to start of line)
        if mods.contains(.command) && (event.specialKey == .delete || event.keyCode == 51) {
            terminalView.send(txt: "\u{15}")
            return nil
        }
        // Cmd+Delete (forward delete) -> Ctrl+K (kill to end of line)
        if mods.contains(.command) && event.keyCode == 117 {
            terminalView.send(txt: "\u{0B}")
            return nil
        }
        // Cmd+Left -> Ctrl+A (move to start of line)
        if mods.contains(.command) && event.specialKey == .leftArrow {
            terminalView.send(txt: "\u{01}")
            return nil
        }
        // Cmd+Right -> Ctrl+E (move to end of line)
        if mods.contains(.command) && event.specialKey == .rightArrow {
            terminalView.send(txt: "\u{05}")
            return nil
        }
        // Option+Backspace -> Ctrl+W (delete word backward)
        if mods.contains(.option) && (event.specialKey == .delete || event.keyCode == 51) {
            terminalView.send(txt: "\u{17}")
            return nil
        }
        return event
    }

    static func applyDefaultTheme(to terminal: LocalProcessTerminalView) {
        terminal.font = NSFont(name: "MesloLGS NF", size: AppSettings.shared.fontSize)
            ?? NSFont(name: "JetBrains Mono", size: AppSettings.shared.fontSize)
            ?? NSFont(name: "SF Mono", size: AppSettings.shared.fontSize)
            ?? NSFont.monospacedSystemFont(ofSize: AppSettings.shared.fontSize, weight: .regular)
        terminal.nativeForegroundColor = NSColor(red: 0.93, green: 0.92, blue: 0.85, alpha: 1)
        terminal.nativeBackgroundColor = NSColor(red: 0.05, green: 0.05, blue: 0.06, alpha: 1)
        terminal.caretColor = NSColor(red: 1.0, green: 0.78, blue: 0.10, alpha: 1)
        terminal.selectedTextBackgroundColor = NSColor(red: 1.0, green: 0.78, blue: 0.10, alpha: 0.3)
    }

    private func buildEnv() -> [String] {
        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["LANG"] = env["LANG"] ?? "en_US.UTF-8"
        env["MINIONSCODE"] = "1"
        return env.map { "\($0.key)=\($0.value)" }
    }

    private func findClaude() -> String {
        let candidates = [
            "/opt/homebrew/bin/claude",
            "/usr/local/bin/claude",
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude/local/bin/claude").path,
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/claude").path,
        ]
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) { return path }
        return "/opt/homebrew/bin/claude"
    }
}
