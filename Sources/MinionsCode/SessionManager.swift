import Foundation

/// Snapshot of a parsed JSONL: sticky cache so we don't re-parse unchanged files every poll tick.
struct JsonlCache: Sendable {
    var fingerprint: String  // "size:mtime" combo
    var usage: TokenUsage
    var model: String?
    var aiTitle: String?
    var cwd: String?
}

@MainActor
@Observable
final class SessionManager {
    static let shared = SessionManager()

    var sessions: [SessionInfo] = []
    var selectedSessionId: String?
    private var timer: Timer?
    private var customNames: [String: String] = [:]
    nonisolated(unsafe) private static var cache: [String: JsonlCache] = [:]
    nonisolated(unsafe) private static let cacheLock = NSLock()

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

    func startPolling(interval: TimeInterval = 5) {
        scan()
        timer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.scan() }
        }
    }

    func scan() {
        let snapshotNames = customNames
        let claudeDir = self.claudeDir
        Task.detached(priority: .utility) {
            let sessions = Self.scanSync(claudeDir: claudeDir, customNames: snapshotNames)
            await MainActor.run { [weak self] in
                guard let self = self else { return }
                self.sessions = sessions
                NotificationManager.shared.observe(sessions: sessions)
            }
        }
    }

    nonisolated static func scanSync(claudeDir: URL, customNames: [String: String]) -> [SessionInfo] {
        let sessionsDir = claudeDir.appendingPathComponent("sessions")
        let projectsDir = claudeDir.appendingPathComponent("projects")

        // Filter for historical sessions: only those active in the last 30 days, and skip
        // outlier files >100MB. This caps the first-scan cost on hosts with thousands of
        // legacy sessions while still surfacing everything a normal user has touched recently.
        let historyHorizon = Date().addingTimeInterval(-30 * 24 * 3600)
        let maxFileBytes = 100 * 1024 * 1024

        // Pass 1 — sessions/ contains one JSON per *currently-running* PID.
        var liveBySessionId: [String: (pid: Int, status: String, version: String, startedAt: Date?, cwd: String)] = [:]
        if let files = try? FileManager.default.contentsOfDirectory(at: sessionsDir, includingPropertiesForKeys: nil) {
            for file in files where file.pathExtension == "json" {
                guard let data = try? Data(contentsOf: file),
                      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
                let pid = json["pid"] as? Int ?? Int(file.deletingPathExtension().lastPathComponent) ?? 0
                guard kill(Int32(pid), 0) == 0 else { continue }
                let sid = json["sessionId"] as? String ?? ""
                let cwd = json["cwd"] as? String ?? ""
                let status = json["status"] as? String ?? "unknown"
                let version = json["version"] as? String ?? ""
                let startedAtMs = json["startedAt"] as? Double
                liveBySessionId[sid] = (pid, status, version, startedAtMs.map { Date(timeIntervalSince1970: $0 / 1000) }, cwd)
            }
        }

        // Pass 2 — walk projects/ for JSONL files modified within the horizon.
        var result: [SessionInfo] = []
        guard let projects = try? FileManager.default.contentsOfDirectory(at: projectsDir, includingPropertiesForKeys: nil) else {
            return []
        }
        for project in projects {
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(atPath: project.path, isDirectory: &isDir), isDir.boolValue else { continue }
            guard let jsonls = try? FileManager.default.contentsOfDirectory(at: project, includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey]) else { continue }

            for url in jsonls where url.pathExtension == "jsonl" {
                let sessionId = url.deletingPathExtension().lastPathComponent
                let live = liveBySessionId[sessionId]
                let isAlive = live != nil

                // Cheap stat-only skip BEFORE we open the file
                guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
                      let size = attrs[.size] as? Int,
                      let mtime = attrs[.modificationDate] as? Date else { continue }
                if size > maxFileBytes { continue }
                if !isAlive && mtime < historyHorizon { continue }

                let (usage, model, aiTitle, lastModified, cwdFromJsonl) = parseUsageStaticWithMeta(jsonlURL: url)
                let cwd = live?.cwd ?? cwdFromJsonl ?? Self.cwdFromProjectName(project.lastPathComponent)
                let cost = Pricing.cost(for: usage, model: model)
                let cacheHitRate: Double = {
                    let total = usage.cacheRead + usage.cacheCreation + usage.totalInput
                    guard total > 0 else { return 0 }
                    return Double(usage.cacheRead) / Double(total)
                }()
                let name = customNames[sessionId] ?? aiTitle ?? Self.shortPathStatic(cwd)

                let startedAt = live?.startedAt ?? lastModified
                result.append(SessionInfo(
                    id: sessionId, pid: live?.pid ?? 0, sessionId: sessionId, name: name,
                    cwd: cwd, status: isAlive ? (live!.status) : "ended", startedAt: startedAt,
                    lastActivityAt: lastModified ?? mtime,
                    version: live?.version ?? "", model: model, usage: usage, cost: cost,
                    cacheHitRate: cacheHitRate, isAlive: isAlive
                ))
            }
        }
        return result.sorted {
            if $0.isAlive != $1.isAlive { return $0.isAlive && !$1.isAlive }
            if $0.isRecentlyActive != $1.isRecentlyActive { return $0.isRecentlyActive && !$1.isRecentlyActive }
            let d0 = $0.lastActivityAt ?? .distantPast
            let d1 = $1.lastActivityAt ?? .distantPast
            return d0 > d1
        }
    }

    nonisolated static func cwdFromProjectName(_ name: String) -> String {
        // Project directory names are shell-encoded paths: "-Users-mjm-projects-MinionsCode" -> "/Users/mjm/projects/MinionsCode"
        // Best-effort decode — leading dash means root, dashes become slashes.
        var s = name
        if s.hasPrefix("-") { s = "/" + s.dropFirst() }
        return s.replacingOccurrences(of: "-", with: "/")
    }

    nonisolated private static func parseUsageStaticWithMeta(jsonlURL: URL) -> (TokenUsage, String?, String?, Date?, String?) {
        let sessionId = jsonlURL.deletingPathExtension().lastPathComponent
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: jsonlURL.path),
              let size = attrs[.size] as? Int,
              let mtime = attrs[.modificationDate] as? Date else {
            return (TokenUsage(), nil, nil, nil, nil)
        }
        let fingerprint = "\(size):\(mtime.timeIntervalSince1970)"

        cacheLock.lock()
        if let cached = cache[sessionId], cached.fingerprint == fingerprint {
            cacheLock.unlock()
            return (cached.usage, cached.model, cached.aiTitle, mtime, cached.cwd)
        }
        cacheLock.unlock()

        guard let content = try? String(contentsOf: jsonlURL, encoding: .utf8) else {
            return (TokenUsage(), nil, nil, mtime, nil)
        }

        var usage = TokenUsage()
        var model: String?
        var aiTitle: String?
        var cwd: String?
        for line in content.components(separatedBy: .newlines) where !line.isEmpty {
            guard let lineData = line.data(using: .utf8),
                  let obj = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any] else { continue }

            if cwd == nil, let c = obj["cwd"] as? String { cwd = c }
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
            if obj["isSidechain"] as? Bool != true {
                usage.messageCount += 1
            }
            if let m = message["model"] as? String { model = m }
        }

        let snapshot = JsonlCache(fingerprint: fingerprint, usage: usage, model: model, aiTitle: aiTitle, cwd: cwd)
        cacheLock.lock()
        cache[sessionId] = snapshot
        cacheLock.unlock()
        return (usage, model, aiTitle, mtime, cwd)
    }

    func renameSession(_ id: String, to name: String) {
        customNames[id] = name.isEmpty ? nil : name
        saveNames()
        if let idx = sessions.firstIndex(where: { $0.id == id }) {
            sessions[idx].name = name.isEmpty ? Self.shortPathStatic(sessions[idx].cwd) : name
        }
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
        Self.shortPathStatic(p)
    }

    nonisolated private static func shortPathStatic(_ p: String) -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        if p.hasPrefix(home) { return "~" + p.dropFirst(home.count) }
        return p
    }
}
