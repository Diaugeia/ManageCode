import AppKit
import SwiftTerm

enum SessionMode {
    case shell
    case claude(resumeId: String?)
}

/// Singleton broker for the "you're read-only" toast.
/// One toast at a time across the whole app. Each new keystroke refreshes
/// the dismissal timer so it stays visible while the user keeps typing,
/// then fades out 2.5s after the last attempt.
@MainActor
@Observable
final class ReadOnlyToastCenter {
    static let shared = ReadOnlyToastCenter()
    var visibleSessionId: String?
    private var dismissTask: Task<Void, Never>?

    func signalAttempt(forSession id: String) {
        visibleSessionId = id
        dismissTask?.cancel()
        dismissTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(2500))
            if !Task.isCancelled {
                self.visibleSessionId = nil
            }
        }
    }

    func dismiss() {
        dismissTask?.cancel()
        visibleSessionId = nil
    }
}

/// Singleton that owns the *single* global NSEvent local monitor.
/// Per-tab monitors caused O(N) closures to run per keystroke; this fixes that.
@MainActor
final class TerminalKeyMonitor {
    static let shared = TerminalKeyMonitor()

    weak var activeSession: TerminalSession?
    nonisolated(unsafe) private var monitor: Any?

    private init() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown]) { [weak self] event in
            guard let self = self,
                  let session = self.activeSession,
                  let window = session.terminalView.window,
                  window.firstResponder === session.terminalView else {
                return event
            }
            return self.handle(event, session: session)
        }
    }

    private func handle(_ event: NSEvent, session: TerminalSession) -> NSEvent? {
        let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let terminal = session.terminalView

        // Read-only: read state freshly from the session, not a cached flag.
        if session.isReadOnly {
            if mods.contains(.command) {
                let chars = event.charactersIgnoringModifiers?.lowercased() ?? ""
                if ["c", "a", "f"].contains(chars) { return event }
            }
            let sk = event.specialKey
            if sk == .pageUp || sk == .pageDown || sk == .home || sk == .end
                || sk == .upArrow || sk == .downArrow {
                return event
            }
            // User tried to type while read-only — surface the toast.
            ReadOnlyToastCenter.shared.signalAttempt(forSession: session.id)
            return nil
        }

        if mods.contains(.command) && (event.specialKey == .delete || event.keyCode == 51) {
            terminal.send(txt: "\u{15}"); return nil
        }
        if mods.contains(.command) && event.keyCode == 117 {
            terminal.send(txt: "\u{0B}"); return nil
        }
        if mods.contains(.command) && event.specialKey == .leftArrow {
            terminal.send(txt: "\u{01}"); return nil
        }
        if mods.contains(.command) && event.specialKey == .rightArrow {
            terminal.send(txt: "\u{05}"); return nil
        }
        if mods.contains(.option) && (event.specialKey == .delete || event.keyCode == 51) {
            terminal.send(txt: "\u{17}"); return nil
        }
        return event
    }
}

@MainActor
final class TerminalSession: @unchecked Sendable {
    let terminalView: LocalProcessTerminalView
    let mode: SessionMode
    let id: String
    let cwd: String
    var isReadOnly: Bool = false
    private(set) var isRunning = false

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
            // Claude sessions default to read-only — like the official `claude --resume`
            // experience, the user can scroll/copy without accidentally typing into a
            // long-running task. Toggle with ⌘E or the lock pill.
            self.isReadOnly = true
            let claudePath = findClaude()
            var args = [String]()
            if let rid = resumeId { args = ["--resume", rid] }
            terminalView.startProcess(executable: claudePath, args: args, environment: env, execName: "claude", currentDirectory: self.cwd)
        }
        isRunning = true
    }

    func sendCommand(_ command: String) {
        terminalView.send(txt: "\(command)\n")
    }

    func toggleReadOnly() {
        setReadOnly(!isReadOnly)
    }

    func setReadOnly(_ ro: Bool) {
        isReadOnly = ro
        // Physically remove keyboard focus when read-only — the terminal can still
        // be scrolled and selected, but cannot receive any keystrokes including
        // those that bypass our NSEvent monitor (IME, paste, etc.).
        guard let window = terminalView.window else { return }
        if ro && window.firstResponder === terminalView {
            window.makeFirstResponder(nil)
        } else if !ro && window.firstResponder !== terminalView {
            window.makeFirstResponder(terminalView)
        }
    }

    /// Activate keyboard handling for this terminal. Call when the user switches tabs.
    func activate() {
        TerminalKeyMonitor.shared.activeSession = self
        if !isReadOnly {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak terminalView] in
                terminalView?.window?.makeFirstResponder(terminalView)
            }
        }
    }

    func terminate() {
        terminalView.process.terminate()
        isRunning = false
    }

    static func applyDefaultTheme(to terminal: LocalProcessTerminalView) {
        let theme = AppSettings.shared.theme
        let translucent = AppSettings.shared.translucentBackground
        terminal.font = NSFont(name: "MesloLGS NF", size: AppSettings.shared.fontSize)
            ?? NSFont(name: "JetBrains Mono", size: AppSettings.shared.fontSize)
            ?? NSFont(name: "SF Mono", size: AppSettings.shared.fontSize)
            ?? NSFont.monospacedSystemFont(ofSize: AppSettings.shared.fontSize, weight: .regular)
        terminal.nativeForegroundColor = theme.foreground
        terminal.nativeBackgroundColor = translucent
            ? theme.background.withAlphaComponent(0.55)
            : theme.background
        terminal.caretColor = theme.primary
        terminal.selectedTextBackgroundColor = theme.primary.withAlphaComponent(0.3)
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
