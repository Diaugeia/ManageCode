import Foundation

struct AISearchResult: Sendable {
    let matchSessionId: String?
    let explanation: String?
}

enum AISearch {
    static func run(query: String, sessions: [[String: String]]) async -> AISearchResult {
        let claudePath = findClaude()
        guard FileManager.default.isExecutableFile(atPath: claudePath) else {
            return AISearchResult(matchSessionId: nil, explanation: "Claude CLI not found")
        }

        let sessionsJSON = (try? JSONSerialization.data(withJSONObject: sessions, options: [])).flatMap {
            String(data: $0, encoding: .utf8)
        } ?? "[]"

        let prompt = """
        You are a session search assistant. Given a user query and a list of Claude Code sessions (id, name, cwd, messages, cost), pick the single best matching session by intent.

        Reply with ONE LINE of pure JSON only, no prose, no code fences:
        {"id":"<sessionId>","reason":"<short explanation, max 60 chars>"}

        If nothing matches, reply: {"id":null,"reason":"<short explanation>"}

        Query: \(query)

        Sessions:
        \(sessionsJSON)
        """

        let process = Process()
        process.executableURL = URL(fileURLWithPath: claudePath)
        process.arguments = ["--print", "--model", "haiku"]

        let inputPipe = Pipe()
        let outputPipe = Pipe()
        process.standardInput = inputPipe
        process.standardOutput = outputPipe
        process.standardError = Pipe()

        do {
            try process.run()
            try inputPipe.fileHandleForWriting.write(contentsOf: prompt.data(using: .utf8) ?? Data())
            try inputPipe.fileHandleForWriting.close()

            let timeoutTask = Task {
                try? await Task.sleep(for: .seconds(30))
                if process.isRunning { process.terminate() }
            }

            let data = outputPipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            timeoutTask.cancel()

            guard let output = String(data: data, encoding: .utf8) else {
                return AISearchResult(matchSessionId: nil, explanation: "Empty response")
            }

            return parseResponse(output)
        } catch {
            return AISearchResult(matchSessionId: nil, explanation: "AI search failed: \(error.localizedDescription)")
        }
    }

    private static func parseResponse(_ raw: String) -> AISearchResult {
        let cleaned = raw
            .replacingOccurrences(of: "```json", with: "")
            .replacingOccurrences(of: "```", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let braceStart = cleaned.firstIndex(of: "{"),
              let braceEnd = cleaned.lastIndex(of: "}") else {
            return AISearchResult(matchSessionId: nil, explanation: cleaned.prefix(80).description)
        }
        let json = String(cleaned[braceStart...braceEnd])
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return AISearchResult(matchSessionId: nil, explanation: "Parse error")
        }
        let id = obj["id"] as? String
        let reason = obj["reason"] as? String
        return AISearchResult(matchSessionId: id, explanation: reason)
    }

    private static func findClaude() -> String {
        let candidates = [
            "/opt/homebrew/bin/claude",
            "/usr/local/bin/claude",
            FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude/local/bin/claude").path,
        ]
        for p in candidates where FileManager.default.isExecutableFile(atPath: p) { return p }
        return "/opt/homebrew/bin/claude"
    }
}
