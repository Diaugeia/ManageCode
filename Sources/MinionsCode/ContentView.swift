import SwiftUI
import AppKit

private let GOLD = Color(red: 1.0, green: 0.78, blue: 0.10)
private let GOLD_DIM = Color(red: 0.85, green: 0.66, blue: 0.08)
private let BG_DARKEST = Color(red: 0.04, green: 0.04, blue: 0.05)
private let BG_DARK = Color(red: 0.07, green: 0.07, blue: 0.08)
private let BG_MID = Color(red: 0.10, green: 0.10, blue: 0.11)
private let TEXT_PRIMARY = Color(red: 0.95, green: 0.93, blue: 0.86)
private let TEXT_DIM = Color.white.opacity(0.5)
private let TEXT_FAINT = Color.white.opacity(0.25)

struct ContentView: View {
    @State private var manager = SessionManager.shared
    @State private var settings = AppSettings.shared
    @State private var terminals: [String: TerminalSession] = [:]
    @State private var activeTerminalId: String?
    @State private var editingName: String?
    @State private var nameInput = ""
    @State private var showingSettings = false
    @State private var searchText = ""
    @State private var aiSearchHint: String?
    @State private var aiSearching = false

    var body: some View {
        HStack(spacing: 0) {
            sidebar
                .frame(width: 290 + (settings.fontSize - 13) * 6)
            Divider().background(Color.white.opacity(0.05))
            terminalPanel
        }
        .background(BG_DARKEST)
        .preferredColorScheme(.dark)
        .environment(\.uiScale, settings.fontSize / 13.0)
        .onAppear {
            manager.startPolling()
            NSApp.activate(ignoringOtherApps: true)
        }
        .onReceive(NotificationCenter.default.publisher(for: .newSession)) { _ in
            newShellSession()
        }
        .sheet(isPresented: $showingSettings) {
            SettingsSheet(isPresented: $showingSettings)
        }
    }

    // MARK: - Sidebar

    private var sidebar: some View {
        VStack(spacing: 0) {
            sidebarHeader
            Divider().background(Color.white.opacity(0.05))
            searchBar
            Divider().background(Color.white.opacity(0.05))
            globalStats
            Divider().background(Color.white.opacity(0.05))
            sessionsList
            Divider().background(Color.white.opacity(0.05))
            sidebarFooter
        }
        .background(BG_DARK)
    }

    private var searchBar: some View {
        VStack(spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: aiSearching ? "sparkles" : "magnifyingglass")
                    .font(.system(size: 11))
                    .foregroundColor(aiSearching ? GOLD : TEXT_DIM)
                TextField("Search sessions or ask AI…", text: $searchText)
                    .textFieldStyle(.plain)
                    .font(.system(size: 12))
                    .foregroundColor(TEXT_PRIMARY)
                    .onSubmit { runAISearchIfNeeded() }
                if !searchText.isEmpty {
                    Button { searchText = ""; aiSearchHint = nil } label: {
                        Image(systemName: "xmark.circle.fill").font(.system(size: 11))
                    }
                    .buttonStyle(.plain).foregroundColor(TEXT_FAINT)
                }
                Button(action: runAISearchIfNeeded) {
                    Image(systemName: "sparkles")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundColor(searchText.isEmpty ? TEXT_FAINT : GOLD)
                }
                .buttonStyle(.plain)
                .help("AI search via Haiku — finds sessions by intent")
                .disabled(searchText.isEmpty)
            }
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 6).fill(BG_MID))

            if let hint = aiSearchHint {
                HStack(spacing: 4) {
                    Image(systemName: "sparkles").font(.system(size: 9))
                    Text(hint).font(.system(size: 10)).lineLimit(2)
                }
                .foregroundColor(GOLD.opacity(0.85))
                .padding(.horizontal, 4)
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 8)
    }

    private var filteredSessions: [SessionInfo] {
        guard !searchText.isEmpty else { return manager.sessions }
        let q = searchText.lowercased()
        return manager.sessions.filter { s in
            s.name.lowercased().contains(q)
                || s.cwd.lowercased().contains(q)
                || (s.model?.lowercased().contains(q) ?? false)
                || s.sessionId.lowercased().contains(q)
        }
    }

    private func runAISearchIfNeeded() {
        guard !searchText.isEmpty, !aiSearching else { return }
        // Only invoke AI if the literal filter returns nothing
        if !filteredSessions.isEmpty { return }
        aiSearching = true
        aiSearchHint = "Asking Haiku…"

        let query = searchText
        let snapshots: [[String: String]] = manager.sessions.prefix(40).map { s in
            [
                "id": s.sessionId,
                "name": s.name,
                "cwd": s.cwd,
                "messages": "\(s.usage.messageCount)",
                "cost": String(format: "%.2f", s.cost),
            ]
        }
        let sessionsForSearch = manager.sessions

        Task {
            let result = await AISearch.run(query: query, sessions: snapshots)
            aiSearching = false
            if let id = result.matchSessionId, let match = sessionsForSearch.first(where: { $0.sessionId == id }) {
                aiSearchHint = "→ \(match.name)"
                searchText = match.name
            } else {
                aiSearchHint = result.explanation ?? "No match found."
            }
        }
    }

    private var sidebarHeader: some View {
        HStack(spacing: 8) {
            MinionDot(size: 14)
            Text("MinionsCode")
                .font(.system(size: 14, weight: .heavy))
                .foregroundColor(TEXT_PRIMARY)
                .tracking(0.3)
            Spacer()
            Button(action: newShellSession) {
                Image(systemName: "plus.circle.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundColor(GOLD)
            }
            .buttonStyle(.plain)
            .help("New shell tab (⌘N) — use the Run Claude menu in the toolbar to launch claude")

            Button { showingSettings = true } label: {
                Image(systemName: "gearshape.fill")
                    .font(.system(size: 14))
                    .foregroundColor(TEXT_DIM)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
    }

    private var globalStats: some View {
        let totals = manager.sessions.reduce(into: TokenTotals()) { acc, s in
            acc.input += s.usage.totalInput
            acc.output += s.usage.totalOutput
            acc.cacheRead += s.usage.cacheRead
            acc.cacheCreation += s.usage.cacheCreation
        }
        let totalCost = manager.totalCost
        let hypotheticalCost = Double(totals.input + totals.cacheRead + totals.cacheCreation) / 1_000_000 * 15
                             + Double(totals.output) / 1_000_000 * 75
        let saved = max(0, hypotheticalCost - totalCost)

        return VStack(spacing: 12) {
            HStack(spacing: 0) {
                StatBlock(label: "ACTIVE", value: "\(manager.activeSessions)", color: GOLD)
                Spacer()
                StatBlock(label: "SPENT", value: fmtCost(totalCost), color: Color.orange)
                Spacer()
                StatBlock(label: "SAVED", value: fmtCost(saved), color: Color.green.opacity(0.85))
            }
            VStack(spacing: 4) {
                TokenRow(label: "Input", tokens: totals.input, price: 15, color: GOLD)
                TokenRow(label: "Cache R", tokens: totals.cacheRead, price: 1.5, color: Color.green.opacity(0.7))
                TokenRow(label: "Cache W", tokens: totals.cacheCreation, price: 18.75, color: Color.orange.opacity(0.8))
                TokenRow(label: "Output", tokens: totals.output, price: 75, color: Color.red.opacity(0.7))
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
    }

    private var sessionsList: some View {
        ScrollView {
            LazyVStack(spacing: 4, pinnedViews: []) {
                if settings.groupByDirectory {
                    let groups = Dictionary(grouping: filteredSessions, by: { shortPath($0.cwd) })
                        .sorted { $0.key < $1.key }
                    ForEach(groups, id: \.key) { group, sessions in
                        SessionGroup(title: group, sessions: sessions, viewModel: viewModel)
                    }
                } else {
                    ForEach(filteredSessions) { session in
                        SessionCard(session: session, viewModel: viewModel)
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
        }
    }

    private var sidebarFooter: some View {
        HStack(spacing: 8) {
            Circle().fill(GOLD).frame(width: 5, height: 5)
            Text("\(manager.activeSessions) sessions")
                .font(.system(size: 10))
                .foregroundColor(TEXT_FAINT)
            Spacer()
            Text("v1.0")
                .font(.system(size: 10))
                .foregroundColor(TEXT_FAINT)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    // MARK: - Terminal Panel

    private var terminalPanel: some View {
        VStack(spacing: 0) {
            tabBar
            Divider().background(Color.white.opacity(0.05))
            if let tid = activeTerminalId, let terminal = terminals[tid] {
                terminalToolbar(for: tid, terminal: terminal)
                IsolatedTerminalView(terminal: terminal)
                    .id(tid)
            } else {
                emptyState
            }
        }
        .background(BG_DARKEST)
    }

    private var tabBar: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    ForEach(orderedTerminalIds, id: \.self) { tid in
                        if let terminal = terminals[tid] {
                            TabChip(
                                terminal: terminal,
                                sessionName: nameForTerminal(terminal),
                                isActive: activeTerminalId == tid,
                                onSelect: { activeTerminalId = tid },
                                onClose: { closeTerminal(tid) }
                            )
                        }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
            }
            HStack(spacing: 4) {
                Button(action: newShellSession) {
                    Image(systemName: "plus")
                        .font(.system(size: 12, weight: .semibold))
                        .frame(width: 26, height: 26)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.04)))
                        .foregroundColor(TEXT_DIM)
                }
                .buttonStyle(.plain)
                .help("New shell tab (⌘T)")
                .keyboardShortcut("t")
            }
            .padding(.trailing, 8)
        }
        .frame(height: 38)
        .background(BG_DARK)
    }

    @State private var orderedTerminalIds: [String] = []

    private func nameForTerminal(_ t: TerminalSession) -> String {
        if case .claude(let resumeId) = t.mode, let rid = resumeId,
           let session = manager.sessions.first(where: { $0.sessionId == rid }) {
            return session.name
        }
        switch t.mode {
        case .shell: return "shell"
        case .claude: return "claude"
        }
    }

    private func closeTerminal(_ id: String) {
        terminals[id]?.terminate()
        terminals.removeValue(forKey: id)
        orderedTerminalIds.removeAll { $0 == id }
        if activeTerminalId == id {
            activeTerminalId = orderedTerminalIds.last
        }
    }

    private func terminalToolbar(for id: String, terminal: TerminalSession) -> some View {
        let session: SessionInfo? = {
            if case .claude(let rid) = terminal.mode, let rid {
                return manager.sessions.first { $0.sessionId == rid }
            }
            return nil
        }()
        let modeLabel: String = {
            switch terminal.mode {
            case .shell: return "shell"
            case .claude: return "claude"
            }
        }()
        return HStack(spacing: 10) {
            Circle().fill(GOLD).frame(width: 6, height: 6)
            Text(session?.name ?? "Terminal")
                .font(.system(size: 12, weight: .semibold, design: .monospaced))
                .foregroundColor(TEXT_PRIMARY)
                .lineLimit(1)
            Text(modeLabel)
                .font(.system(size: 9, weight: .bold))
                .padding(.horizontal, 6).padding(.vertical, 2)
                .background(Capsule().fill(GOLD.opacity(0.15)))
                .foregroundColor(GOLD)

            ReadOnlyToggle(terminal: terminal)

            Spacer()
            if let s = session {
                Pill(label: "In", value: fmtTokens(s.usage.totalInput), color: GOLD)
                Pill(label: "Cache", value: fmtPct(s.cacheHitRate), color: Color.green.opacity(0.85))
                Pill(label: "$", value: fmtCost(s.cost), color: Color.orange)
            }
            if case .shell = terminal.mode {
                ClaudeLaunchMenu(terminal: terminal)
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 8)
        .background(BG_DARK)
    }

    private var emptyState: some View {
        VStack(spacing: 18) {
            MinionDot(size: 56).opacity(0.6)
            VStack(spacing: 4) {
                Text("Welcome to MinionsCode")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundColor(TEXT_PRIMARY)
                Text("A terminal with Claude Code superpowers")
                    .font(.system(size: 12))
                    .foregroundColor(TEXT_DIM)
            }
            HStack(spacing: 10) {
                Button(action: newShellSession) {
                    Label("New Shell", systemImage: "terminal")
                        .font(.system(size: 13, weight: .semibold))
                        .padding(.horizontal, 16).padding(.vertical, 9)
                        .background(RoundedRectangle(cornerRadius: 8).fill(GOLD.opacity(0.12)))
                        .foregroundColor(GOLD)
                }
                .buttonStyle(.plain)
                .keyboardShortcut("n")
                Button(action: newClaudeSession) {
                    Label("New Claude", systemImage: "sparkles")
                        .font(.system(size: 13, weight: .semibold))
                        .padding(.horizontal, 16).padding(.vertical, 9)
                        .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.06)))
                        .foregroundColor(TEXT_PRIMARY)
                }
                .buttonStyle(.plain)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Actions

    private var viewModel: SessionViewModel {
        SessionViewModel(
            activeId: activeTerminalId,
            editingId: editingName,
            nameInput: $nameInput,
            onSelect: selectSession,
            onResume: resumeSession,
            onRename: startRename,
            onCommitRename: commitRename
        )
    }

    private func newShellSession() {
        let cwd = manager.selectedSession?.cwd ?? FileManager.default.homeDirectoryForCurrentUser.path
        let terminal = TerminalSession(mode: .shell, cwd: cwd)
        terminals[terminal.id] = terminal
        orderedTerminalIds.append(terminal.id)
        activeTerminalId = terminal.id
    }

    private func newClaudeSession() {
        let terminal = TerminalSession(mode: .claude(resumeId: nil))
        terminals[terminal.id] = terminal
        orderedTerminalIds.append(terminal.id)
        activeTerminalId = terminal.id
    }

    private func selectSession(_ session: SessionInfo) {
        if terminals[session.id] != nil {
            activeTerminalId = session.id
        } else {
            resumeSession(session)
        }
    }

    private func resumeSession(_ session: SessionInfo) {
        if terminals[session.id] != nil {
            activeTerminalId = session.id
            return
        }
        let terminal = TerminalSession(mode: .claude(resumeId: session.sessionId), cwd: session.cwd)
        terminals[session.id] = terminal
        orderedTerminalIds.append(session.id)
        activeTerminalId = session.id
    }

    private func startRename(_ session: SessionInfo) {
        nameInput = session.name
        editingName = session.id
    }

    private func commitRename(_ session: SessionInfo) {
        manager.renameSession(session.id, to: nameInput)
        editingName = nil
    }
}

/// Isolates the terminal NSView from parent SwiftUI re-renders.
/// Equatable on terminal id so updateNSView is a no-op when sessions update.
struct IsolatedTerminalView: View {
    let terminal: TerminalSession

    var body: some View {
        TerminalViewRepresentable(terminalView: terminal.terminalView)
    }
}

struct ReadOnlyToggle: View {
    let terminal: TerminalSession
    @State private var readOnly = false

    var body: some View {
        Button {
            terminal.toggleReadOnly()
            readOnly = terminal.isReadOnly
        } label: {
            HStack(spacing: 3) {
                Image(systemName: readOnly ? "lock.fill" : "pencil.circle.fill")
                    .font(.system(size: 10))
                Text(readOnly ? "Read-only" : "Editing")
                    .font(.system(size: 10, weight: .semibold))
            }
            .padding(.horizontal, 7).padding(.vertical, 3)
            .background(Capsule().fill(readOnly ? Color.gray.opacity(0.18) : Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.15)))
            .foregroundColor(readOnly ? .white.opacity(0.7) : Color(red: 1.0, green: 0.78, blue: 0.10))
        }
        .buttonStyle(.plain)
        .help(readOnly ? "Read-only mode — click to enable editing (⌘E)" : "Editing mode — click to lock (⌘E)")
        .keyboardShortcut("e")
        .onAppear { readOnly = terminal.isReadOnly }
    }
}

struct ClaudeLaunchMenu: View {
    let terminal: TerminalSession

    var body: some View {
        Menu {
            Section("Quick start") {
                Button { terminal.sendCommand("claude") } label: {
                    Label("claude", systemImage: "sparkles")
                }
                Button { terminal.sendCommand("claude --effort max") } label: {
                    Label("claude — effort max", systemImage: "bolt.fill")
                }
                Button { terminal.sendCommand("claude --permission-mode bypassPermissions") } label: {
                    Label("claude — bypass permissions", systemImage: "lock.open")
                }
                Button { terminal.sendCommand("claude --effort max --permission-mode bypassPermissions") } label: {
                    Label("claude — max + bypass", systemImage: "flame.fill")
                }
            }
            Section("Models") {
                Button { terminal.sendCommand("claude --model opus") } label: { Text("claude — opus") }
                Button { terminal.sendCommand("claude --model sonnet") } label: { Text("claude — sonnet") }
                Button { terminal.sendCommand("claude --model haiku") } label: { Text("claude — haiku") }
            }
            Section("Resume") {
                Button { terminal.sendCommand("claude --resume") } label: {
                    Label("claude — resume picker", systemImage: "list.bullet")
                }
                Button { terminal.sendCommand("claude --continue") } label: {
                    Label("claude — continue last", systemImage: "arrow.clockwise")
                }
            }
            Section("Advanced") {
                Button { terminal.sendCommand("claude --help") } label: { Text("claude — help") }
                Button { terminal.sendCommand("claude doctor") } label: { Text("claude doctor") }
                Button { terminal.sendCommand("claude /cost") } label: { Text("claude /cost") }
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "sparkles")
                Text("Run Claude")
                Image(systemName: "chevron.down").font(.system(size: 8))
            }
            .font(.system(size: 11, weight: .semibold))
            .padding(.horizontal, 10).padding(.vertical, 5)
            .background(Capsule().fill(Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.15)))
            .foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
    }
}

struct TabChip: View {
    let terminal: TerminalSession
    let sessionName: String
    let isActive: Bool
    let onSelect: () -> Void
    let onClose: () -> Void
    @State private var isHovering = false

    private var modeIcon: String {
        switch terminal.mode {
        case .shell: return "terminal"
        case .claude: return "sparkles"
        }
    }

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: modeIcon)
                .font(.system(size: 10))
                .foregroundColor(isActive ? Color(red: 1.0, green: 0.78, blue: 0.10) : .white.opacity(0.5))
            Text(sessionName)
                .font(.system(size: 11, weight: isActive ? .semibold : .regular))
                .foregroundColor(isActive ? .white : .white.opacity(0.6))
                .lineLimit(1)
                .truncationMode(.tail)
            Button(action: onClose) {
                Image(systemName: "xmark")
                    .font(.system(size: 8, weight: .bold))
                    .frame(width: 14, height: 14)
                    .background(Circle().fill(isHovering ? Color.white.opacity(0.1) : Color.clear))
                    .foregroundColor(.white.opacity(isHovering ? 0.8 : 0.4))
            }
            .buttonStyle(.plain)
            .onHover { isHovering = $0 }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .frame(minWidth: 100, maxWidth: 200)
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(isActive ? Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.12) : Color.white.opacity(0.03))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(isActive ? Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.4) : Color.clear, lineWidth: 1)
                )
        )
        .contentShape(Rectangle())
        .onTapGesture { onSelect() }
    }
}

struct SessionViewModel {
    let activeId: String?
    let editingId: String?
    let nameInput: Binding<String>
    let onSelect: (SessionInfo) -> Void
    let onResume: (SessionInfo) -> Void
    let onRename: (SessionInfo) -> Void
    let onCommitRename: (SessionInfo) -> Void
}

struct TokenTotals {
    var input = 0
    var output = 0
    var cacheRead = 0
    var cacheCreation = 0
}

struct MinionDot: View {
    let size: CGFloat
    var body: some View {
        ZStack {
            Circle()
                .fill(LinearGradient(
                    colors: [Color(red: 1, green: 0.85, blue: 0.2), Color(red: 0.95, green: 0.65, blue: 0.05)],
                    startPoint: .topLeading, endPoint: .bottomTrailing))
            Circle()
                .stroke(Color.black.opacity(0.6), lineWidth: max(1, size * 0.06))
            HStack(spacing: size * 0.08) {
                Circle().fill(Color.black).frame(width: size * 0.18, height: size * 0.18)
                Circle().fill(Color.black).frame(width: size * 0.18, height: size * 0.18)
            }
            .offset(y: -size * 0.05)
        }
        .frame(width: size, height: size)
    }
}

struct StatBlock: View {
    let label: String
    let value: String
    let color: Color

    var body: some View {
        VStack(spacing: 2) {
            Text(value)
                .font(.system(size: 16, weight: .heavy, design: .monospaced))
                .foregroundColor(color)
            Text(label)
                .font(.system(size: 9, weight: .bold))
                .foregroundColor(.white.opacity(0.35))
                .tracking(0.5)
        }
    }
}

struct TokenRow: View {
    let label: String
    let tokens: Int
    let price: Double
    let color: Color

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(color).frame(width: 4, height: 4)
            Text(label)
                .font(.system(size: 10, weight: .medium))
                .foregroundColor(.white.opacity(0.55))
                .frame(width: 60, alignment: .leading)
            Text(fmtTokens(tokens))
                .font(.system(size: 10, weight: .semibold, design: .monospaced))
                .foregroundColor(.white.opacity(0.7))
            Spacer()
            Text(fmtCost(Double(tokens) / 1_000_000 * price))
                .font(.system(size: 10, weight: .semibold, design: .monospaced))
                .foregroundColor(color.opacity(0.95))
        }
    }
}

struct SessionGroup: View {
    let title: String
    let sessions: [SessionInfo]
    let viewModel: SessionViewModel
    @State private var expanded = true
    @State private var hovering = false

    var body: some View {
        VStack(spacing: 2) {
            HStack(spacing: 6) {
                Image(systemName: expanded ? "chevron.down" : "chevron.right")
                    .font(.system(size: 9, weight: .bold))
                    .frame(width: 12)
                Text(title)
                    .font(.system(size: 10, weight: .heavy))
                    .tracking(0.5)
                    .lineLimit(1)
                Spacer()
                Text("\(sessions.count)")
                    .font(.system(size: 9, weight: .semibold, design: .monospaced))
                    .foregroundColor(.white.opacity(0.4))
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 4)
                    .fill(hovering ? Color.white.opacity(0.04) : Color.clear)
            )
            .foregroundColor(.white.opacity(0.55))
            .contentShape(Rectangle())
            .onHover { hovering = $0 }
            .onTapGesture {
                withAnimation(.easeInOut(duration: 0.15)) { expanded.toggle() }
            }

            if expanded {
                ForEach(sessions) { session in
                    SessionCard(session: session, viewModel: viewModel)
                }
            }
        }
    }
}

struct SessionCard: View {
    let session: SessionInfo
    let viewModel: SessionViewModel
    @Environment(\.uiScale) private var scale
    private var isActive: Bool { viewModel.activeId == session.id }
    private var isEditing: Bool { viewModel.editingId == session.id }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 6) {
                Circle()
                    .fill(session.isAlive ? (session.status == "busy" ? Color(red: 1.0, green: 0.78, blue: 0.10) : Color.green) : Color.gray.opacity(0.4))
                    .frame(width: 6, height: 6)
                if isEditing {
                    TextField("Name", text: viewModel.nameInput, onCommit: { viewModel.onCommitRename(session) })
                        .textFieldStyle(.plain)
                        .scaledFont(11, weight: .semibold)
                        .foregroundColor(.white)
                } else {
                    Text(session.name)
                        .scaledFont(11, weight: .semibold)
                        .foregroundColor(.white.opacity(0.92))
                        .lineLimit(1)
                }
                Spacer()
                Text(fmtCost(session.cost))
                    .scaledFont(10, weight: .bold, design: .monospaced)
                    .foregroundColor(Color.orange.opacity(0.85))
            }
            HStack(spacing: 8) {
                if let model = session.model {
                    Text(model.replacingOccurrences(of: "claude-", with: "")
                            .replacingOccurrences(of: "opus-4-7", with: "opus")
                            .replacingOccurrences(of: "opus-4.7", with: "opus"))
                        .foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.7))
                }
                Text("\(session.usage.messageCount)msg")
                if let started = session.startedAt {
                    Text(fmtDuration(Date().timeIntervalSince(started)))
                }
                Spacer()
                Text("⚡\(fmtPct(session.cacheHitRate))")
                    .foregroundColor(session.cacheHitRate > 0.7 ? .green.opacity(0.7) : .yellow.opacity(0.7))
            }
            .scaledFont(9, design: .monospaced)
            .foregroundColor(.white.opacity(0.32))
        }
        .padding(.horizontal, 10).padding(.vertical, 8)
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(isActive ? Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.08) : Color.white.opacity(0.02))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(isActive ? Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.4) : Color.clear, lineWidth: 1)
                )
        )
        .contentShape(Rectangle())
        .onTapGesture { viewModel.onSelect(session) }
        .contextMenu {
            Button("Resume in Terminal") { viewModel.onResume(session) }
            Button("Rename...") { viewModel.onRename(session) }
            Divider()
            Button("Copy Session ID") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(session.sessionId, forType: .string)
            }
            Button("Open Folder in Finder") {
                NSWorkspace.shared.open(URL(fileURLWithPath: session.cwd))
            }
        }
    }
}

struct Pill: View {
    let label: String
    let value: String
    let color: Color
    var body: some View {
        HStack(spacing: 3) {
            Text(label).font(.system(size: 9)).foregroundColor(.white.opacity(0.4))
            Text(value).font(.system(size: 10, weight: .semibold, design: .monospaced)).foregroundColor(color)
        }
        .padding(.horizontal, 7).padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: 4).fill(color.opacity(0.1)))
    }
}

struct SettingsSheet: View {
    @Binding var isPresented: Bool
    @State private var settings = AppSettings.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Settings").font(.system(size: 16, weight: .bold))
                Spacer()
                Button { isPresented = false } label: {
                    Image(systemName: "xmark.circle.fill").font(.system(size: 16))
                }
                .buttonStyle(.plain).foregroundColor(.white.opacity(0.5))
            }
            Divider()
            VStack(alignment: .leading, spacing: 10) {
                Text("Appearance").font(.system(size: 11, weight: .bold)).tracking(0.5).foregroundColor(.white.opacity(0.5))
                HStack {
                    Text("Font Size")
                    Spacer()
                    Stepper("\(Int(settings.fontSize))pt", value: $settings.fontSize, in: 9...22, step: 1)
                        .labelsHidden()
                    Text("\(Int(settings.fontSize))pt").frame(width: 40)
                        .font(.system(.body, design: .monospaced))
                }
                HStack {
                    Text("Theme")
                    Spacer()
                    Picker("", selection: $settings.theme) {
                        ForEach(Theme.allCases, id: \.self) { t in Text(t.displayName).tag(t) }
                    }
                    .labelsHidden().frame(width: 200)
                }
            }
            Divider()
            VStack(alignment: .leading, spacing: 10) {
                Text("Behavior").font(.system(size: 11, weight: .bold)).tracking(0.5).foregroundColor(.white.opacity(0.5))
                Toggle("Group sessions by directory", isOn: $settings.groupByDirectory)
                Toggle("Notify when Claude finishes", isOn: $settings.notificationsEnabled)
                Toggle("Play sound on completion", isOn: $settings.soundEnabled).disabled(!settings.notificationsEnabled)
            }
            Spacer()
        }
        .padding(20)
        .frame(width: 420, height: 360)
        .background(BG_DARK)
        .preferredColorScheme(.dark)
    }
}

extension Color {
    init(hex: UInt32) {
        let r = Double((hex >> 16) & 0xFF) / 255
        let g = Double((hex >> 8) & 0xFF) / 255
        let b = Double(hex & 0xFF) / 255
        self.init(red: r, green: g, blue: b)
    }
}

private struct UIScaleKey: EnvironmentKey {
    static let defaultValue: CGFloat = 1.0
}

extension EnvironmentValues {
    var uiScale: CGFloat {
        get { self[UIScaleKey.self] }
        set { self[UIScaleKey.self] = newValue }
    }
}

extension View {
    func scaledFont(_ size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default) -> some View {
        modifier(ScaledFontModifier(size: size, weight: weight, design: design))
    }
}

struct ScaledFontModifier: ViewModifier {
    let size: CGFloat
    let weight: Font.Weight
    let design: Font.Design
    @Environment(\.uiScale) private var scale

    func body(content: Content) -> some View {
        content.font(.system(size: size * scale, weight: weight, design: design))
    }
}

func shortPath(_ p: String) -> String {
    let home = FileManager.default.homeDirectoryForCurrentUser.path
    if p.hasPrefix(home) { return "~" + p.dropFirst(home.count) }
    return p
}
func fmtTokens(_ n: Int) -> String {
    if n >= 1_000_000 { return String(format: "%.1fM", Double(n) / 1_000_000) }
    if n >= 1_000 { return String(format: "%.0fK", Double(n) / 1_000) }
    return "\(n)"
}
func fmtCost(_ n: Double) -> String {
    if n >= 1000 { return String(format: "$%.0f", n) }
    if n >= 10 { return String(format: "$%.1f", n) }
    if n >= 1 { return String(format: "$%.2f", n) }
    return String(format: "$%.3f", n)
}
func fmtPct(_ n: Double) -> String { String(format: "%.0f%%", n * 100) }
func fmtDuration(_ seconds: TimeInterval) -> String {
    let mins = Int(seconds / 60)
    if mins < 60 { return "\(mins)m" }
    let hrs = mins / 60
    if hrs < 24 { return "\(hrs)h\(mins % 60)m" }
    let days = hrs / 24
    return "\(days)d\(hrs % 24)h"
}
