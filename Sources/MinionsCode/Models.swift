import Foundation

struct SessionInfo: Identifiable, Hashable, Sendable {
    let id: String
    let pid: Int
    let sessionId: String
    var name: String
    let cwd: String
    let status: String
    let startedAt: Date?
    let version: String
    let model: String?
    let usage: TokenUsage
    let cost: Double
    let cacheHitRate: Double
    var isAlive: Bool

    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    static func == (lhs: SessionInfo, rhs: SessionInfo) -> Bool { lhs.id == rhs.id }
}

struct TokenUsage: Sendable {
    var totalInput: Int = 0
    var totalOutput: Int = 0
    var cacheRead: Int = 0
    var cacheCreation: Int = 0
    var messageCount: Int = 0
}

enum Pricing {
    static func cost(for usage: TokenUsage, model: String?) -> Double {
        let p = pricing(for: model)
        return Double(usage.totalInput) / 1_000_000 * p.0
             + Double(usage.totalOutput) / 1_000_000 * p.1
             + Double(usage.cacheRead) / 1_000_000 * p.2
             + Double(usage.cacheCreation) / 1_000_000 * p.3
    }

    static func pricing(for model: String?) -> (Double, Double, Double, Double) {
        guard let m = model?.lowercased() else { return (15, 75, 1.5, 18.75) }
        if m.contains("sonnet") { return (3, 15, 0.3, 3.75) }
        if m.contains("haiku") { return (0.8, 4, 0.08, 1) }
        return (15, 75, 1.5, 18.75)
    }
}
