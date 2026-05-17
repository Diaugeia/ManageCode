import Foundation

@MainActor
@Observable
final class SessionManager {
    static let shared = SessionManager()

    var sessions: [SessionInfo] = []
    var selectedSessionId: String?
    private var timer: Timer?
    private var customNames: [String: String] = [:]

    private let claudeDir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude")
    private let namesFile: URL

    init() {
        namesFile = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".minionscode")
            .appendingPathComponent("session-names.json")
        loadNames()
    }

    var selectedSession: SessionInfo? {
        sessions.first { $0.id == selectedSessionId }
    }

    var totalCost: Double { sessions.reduce(0) { $0 + $1.cost } }
    var activeSessions: Int { sessions.filter(\.isAlive).count }

    func startPolling(interval: TimeInterval = 3) {
        scan()
        timer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.scan() }
        }
    }

    func scan() {
        let sessionsDir = claudeDir.appendingPathComponent("sessions")
        let projectsDir = claudeDir.appendingPathComponent("projects")

        guard let files = try? FileManager.default.contentsOfDirectory(at: sessionsDir, includingPropertiesForKeys: nil) else {
            sessions = []
            return
        }

        var result: [SessionInfo] = []

        for file in files where file.pathExtension == "json" {
            guard let data = try? Data(contentsOf: file),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }

            let pid = json["pid"] as? Int ?? Int(file.deletingPathExtension().lastPathComponent) ?? 0
            let alive = kill(Int32(pid), 0) == 0
            let sessionId = json["sessionId"] as? String ?? ""
            let cwd = json["cwd"] as? String ?? ""
            let status = json["status"] as? String ?? "unknown"
            let version = json["version"] as? String ?? ""
            let startedAtMs = json["startedAt"] as? Double
            let startedAt = startedAtMs.map { Date(timeIntervalSince1970: $0 / 1000) }

            let (usage, model, aiTitle) = parseUsage(sessionId: sessionId, projectsDir: projectsDir)
            let cost = Pricing.cost(for: usage, model: model)
            let cacheHitRate: Double = {
                let total = usage.cacheRead + usage.cacheCreation + usage.totalInput
                guard total > 0 else { return 0 }
                return Double(usage.cacheRead) / Double(total)
            }()

            let name = customNames[sessionId] ?? aiTitle ?? shortPath(cwd)

            result.append(SessionInfo(
                id: sessionId, pid: pid, sessionId: sessionId, name: name,
                cwd: cwd, status: alive ? status : "dead", startedAt: startedAt,
                version: version, model: model, usage: usage, cost: cost,
                cacheHitRate: cacheHitRate, isAlive: alive
            ))
        }

        sessions = result.sorted { ($0.isAlive ? 0 : 1, -$0.cost) < ($1.isAlive ? 0 : 1, -$1.cost) }
    }

    func renameSession(_ id: String, to name: String) {
        customNames[id] = name.isEmpty ? nil : name
        saveNames()
        if let idx = sessions.firstIndex(where: { $0.id == id }) {
            sessions[idx].name = name.isEmpty ? shortPath(sessions[idx].cwd) : name
        }
    }

    private func parseUsage(sessionId: String, projectsDir: URL) -> (TokenUsage, String?, String?) {
        var usage = TokenUsage()
        var model: String?
        var aiTitle: String?

        guard let projects = try? FileManager.default.contentsOfDirectory(at: projectsDir, includingPropertiesForKeys: nil) else {
            return (usage, model, aiTitle)
        }

        for project in projects {
            let jsonlFile = project.appendingPathComponent("\(sessionId).jsonl")
            guard FileManager.default.fileExists(atPath: jsonlFile.path),
                  let content = try? String(contentsOf: jsonlFile, encoding: .utf8) else { continue }

            for line in content.components(separatedBy: .newlines) where !line.isEmpty {
                guard let lineData = line.data(using: .utf8),
                      let obj = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any] else { continue }

                if obj["type"] as? String == "ai-title" {
                    aiTitle = obj["aiTitle"] as? String
                }

                guard obj["type"] as? String == "assistant",
                      let message = obj["message"] as? [String: Any],
                      let u = message["usage"] as? [String: Any] else { continue }

                usage.totalInput += u["input_tokens"] as? Int ?? 0
                usage.totalOutput += u["output_tokens"] as? Int ?? 0
                usage.cacheRead += u["cache_read_input_tokens"] as? Int ?? 0
                usage.cacheCreation += u["cache_creation_input_tokens"] as? Int ?? 0
                usage.messageCount += 1
                if let m = message["model"] as? String { model = m }
            }
            break
        }
        return (usage, model, aiTitle)
    }

    private func loadNames() {
        guard let data = try? Data(contentsOf: namesFile),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: String] else { return }
        customNames = dict
    }

    private func saveNames() {
        let dir = namesFile.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        if let data = try? JSONSerialization.data(withJSONObject: customNames, options: .prettyPrinted) {
            try? data.write(to: namesFile)
        }
    }

    private func shortPath(_ p: String) -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        if p.hasPrefix(home) { return "~" + p.dropFirst(home.count) }
        return p
    }
}
