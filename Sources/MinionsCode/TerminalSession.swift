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
            let claudePath = findClaude()
            var args = [String]()
            if let rid = resumeId { args = ["--resume", rid] }
            terminalView.startProcess(executable: claudePath, args: args, environment: env, execName: "claude", currentDirectory: self.cwd)
        }
        isRunning = true
    }

    func sendClaudeCommand() {
        terminalView.send(txt: "claude\n")
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
