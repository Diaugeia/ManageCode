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
    @State private var sidebarCollapsed = true
    @State private var sidebarWidth: CGFloat = 320
    @State private var orderedTerminalIds: [String] = []
    @State private var showingCloseConfirm = false
    @State private var pendingCloseId: String?

    private var sidebarTargetWidth: CGFloat {
        sidebarCollapsed ? 0 : sidebarWidth
    }

    var body: some View {
        HStack(spacing: 0) {
            // Terminal panel takes priority — fills available space
            terminalPanel
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            if !sidebarCollapsed {
                Divider().background(Color.white.opacity(0.05))
                SidebarResizer(width: $sidebarWidth)
                sidebar
                    .frame(width: sidebarWidth)
                    .transition(.move(edge: .trailing))
            }
        }
        .background(
            ZStack {
                if settings.translucentBackground {
                    VisualEffectBackground(material: .underWindowBackground, blendingMode: .behindWindow)
                }
                BG_DARKEST.opacity(settings.translucentBackground ? 0.55 : 1)
            }
        )
        .preferredColorScheme(.dark)
        .environment(\.uiScale, settings.fontSize / 13.0)
        .onAppear {
            manager.startPolling()
            NSApp.activate(ignoringOtherApps: true)
            applyWindowTranslucency()
            if terminals.isEmpty { newShellSession() }
        }
        .onChange(of: settings.theme) { _, _ in
            for t in terminals.values {
                TerminalSession.applyDefaultTheme(to: t.terminalView)
            }
        }
        .onChange(of: settings.fontSize) { _, _ in
            for t in terminals.values {
                TerminalSession.applyDefaultTheme(to: t.terminalView)
            }
        }
        .onChange(of: settings.translucentBackground) { _, _ in
            applyWindowTranslucency()
            for t in terminals.values {
                TerminalSession.applyDefaultTheme(to: t.terminalView)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .newSession)) { _ in
            newShellSession()
        }
        .onReceive(NotificationCenter.default.publisher(for: .toggleSidebar)) { _ in
            withAnimation(.easeInOut(duration: 0.2)) { sidebarCollapsed.toggle() }
        }
        .onReceive(NotificationCenter.default.publisher(for: .closeActiveTab)) { _ in
            if let tid = activeTerminalId { closeTerminal(tid) }
        }
        .onReceive(NotificationCenter.default.publisher(for: .zoomIn)) { _ in
            settings.fontSize = min(22, settings.fontSize + 1)
        }
        .onReceive(NotificationCenter.default.publisher(for: .zoomOut)) { _ in
            settings.fontSize = max(9, settings.fontSize - 1)
        }
        .onReceive(NotificationCenter.default.publisher(for: .zoomReset)) { _ in
            settings.fontSize = 13
        }
        .onReceive(NotificationCenter.default.publisher(for: .selectTab)) { note in
            if let i = note.object as? Int, i >= 0 && i < orderedTerminalIds.count {
                let tid = orderedTerminalIds[i]
                activeTerminalId = tid
                terminals[tid]?.activate()
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .nextTab)) { _ in
            switchTab(offset: 1)
        }
        .onReceive(NotificationCenter.default.publisher(for: .prevTab)) { _ in
            switchTab(offset: -1)
        }
        .sheet(isPresented: $showingSettings) {
            SettingsSheet(isPresented: $showingSettings)
        }
        .alert("Close this tab?", isPresented: $showingCloseConfirm, presenting: pendingCloseId) { id in
            Button("Cancel", role: .cancel) { pendingCloseId = nil }
            Button("Close", role: .destructive) {
                if let id = pendingCloseId { reallyCloseTerminal(id) }
                pendingCloseId = nil
            }
        } message: { _ in
            Text("This terminal has a process running. Closing will terminate it.")
        }
        .accentColor(GOLD)
        .tint(GOLD)
    }

    private func switchTab(offset: Int) {
        guard let tid = activeTerminalId,
              let idx = orderedTerminalIds.firstIndex(of: tid),
              !orderedTerminalIds.isEmpty else { return }
        let n = orderedTerminalIds.count
        let next = ((idx + offset) % n + n) % n
        let nextId = orderedTerminalIds[next]
        activeTerminalId = nextId
        terminals[nextId]?.activate()
    }

    private func applyWindowTranslucency() {
        guard let window = NSApp.windows.first else { return }
        if settings.translucentBackground {
            window.isOpaque = false
            window.backgroundColor = .clear
            window.titlebarAppearsTransparent = true
        } else {
            window.isOpaque = true
            window.backgroundColor = NSColor(red: 0.04, green: 0.04, blue: 0.05, alpha: 1)
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
            Text("Sessions")
                .font(.system(size: 13, weight: .heavy))
                .foregroundColor(TEXT_PRIMARY)
                .tracking(0.3)
            Spacer()
            Menu {
                Button("Delete empty sessions") { deleteEmptySessions() }
                Button("Refresh now") { manager.scan() }
            } label: {
                Image(systemName: "ellipsis.circle")
                    .font(.system(size: 13))
                    .foregroundColor(TEXT_DIM)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()

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
            statusStrip
            tabBar
            Divider().background(Color.white.opacity(0.05))
            ZStack {
                if let tid = activeTerminalId, let terminal = terminals[tid] {
                    VStack(spacing: 0) {
                        terminalToolbar(for: tid, terminal: terminal)
                        IsolatedTerminalView(terminal: terminal)
                            .id(tid)
                    }
                } else {
                    emptyState
                }
                ReadOnlyToastOverlay(activeTerminalId: activeTerminalId, onEnableEditing: {
                    if let tid = activeTerminalId, let t = terminals[tid] {
                        t.setReadOnly(false)
                    }
                })
            }
        }
        .background(BG_DARKEST)
    }

    private var statusStrip: some View {
        HStack(spacing: 12) {
            HStack(spacing: 6) {
                MinionDot(size: 14)
                Text("MinionsCode")
                    .font(.system(size: 12, weight: .heavy))
                    .foregroundColor(TEXT_PRIMARY)
                    .tracking(0.3)
            }
            Spacer()
            HStack(spacing: 10) {
                statusBadge(label: "active", value: "\(manager.activeSessions)", tint: GOLD)
                statusBadge(label: "tracked", value: "\(manager.sessions.count)", tint: TEXT_DIM)
                statusBadge(label: "spent", value: fmtCost(manager.totalCost), tint: Color.orange)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 7)
        .background(BG_DARK)
    }

    private func statusBadge(label: String, value: String, tint: Color) -> some View {
        HStack(spacing: 4) {
            Text(label.uppercased())
                .font(.system(size: 8, weight: .bold))
                .tracking(0.5)
                .foregroundColor(.white.opacity(0.35))
            Text(value)
                .font(.system(size: 11, weight: .semibold, design: .monospaced))
                .foregroundColor(tint)
        }
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
                                onSelect: {
                                    if activeTerminalId != tid {
                                        activeTerminalId = tid
                                        terminal.activate()
                                    }
                                },
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

                Button {
                    NotificationCenter.default.post(name: .toggleSidebar, object: nil)
                } label: {
                    Image(systemName: sidebarCollapsed ? "sidebar.right" : "sidebar.right.fill")
                        .font(.system(size: 12, weight: .semibold))
                        .frame(width: 26, height: 26)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.04)))
                        .foregroundColor(sidebarCollapsed ? TEXT_DIM : GOLD)
                }
                .buttonStyle(.plain)
                .help(sidebarCollapsed ? "Show Claude sessions panel (⌘\\)" : "Hide panel (⌘\\)")
                .keyboardShortcut("\\")
            }
            .padding(.trailing, 8)
        }
        .frame(height: 38)
        .background(BG_DARK)
    }

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
        // Always confirm — protects against ⌘W misfire and hides the
        // edge case where the user has work in progress.
        pendingCloseId = id
        showingCloseConfirm = true
    }

    private func reallyCloseTerminal(_ id: String) {
        terminals[id]?.terminate()
        terminals.removeValue(forKey: id)
        orderedTerminalIds.removeAll { $0 == id }
        if activeTerminalId == id {
            activeTerminalId = orderedTerminalIds.last
            if let next = activeTerminalId, let t = terminals[next] { t.activate() }
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
            CdFolderButton(terminal: terminal)

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
        terminal.activate()
    }

    private func newClaudeSession() {
        let terminal = TerminalSession(mode: .claude(resumeId: nil))
        terminals[terminal.id] = terminal
        orderedTerminalIds.append(terminal.id)
        activeTerminalId = terminal.id
        terminal.activate()
    }

    private func selectSession(_ session: SessionInfo) {
        if let existing = terminals[session.id] {
            activeTerminalId = session.id
            existing.activate()
        } else {
            resumeSession(session)
        }
    }

    private func resumeSession(_ session: SessionInfo) {
        if let existing = terminals[session.id] {
            activeTerminalId = session.id
            existing.activate()
            return
        }
        let terminal = TerminalSession(mode: .claude(resumeId: session.sessionId), cwd: session.cwd)
        terminals[session.id] = terminal
        orderedTerminalIds.append(session.id)
        activeTerminalId = session.id
        terminal.activate()
    }

    private func deleteEmptySessions() {
        // Close any tabs whose sessions had 0 user messages — heuristic for "never used".
        let toClose = terminals.values.compactMap { t -> String? in
            if case .claude(let rid) = t.mode, let rid {
                if let s = manager.sessions.first(where: { $0.sessionId == rid }), s.usage.messageCount == 0 {
                    return t.id
                }
            }
            return nil
        }
        for id in toClose { closeTerminal(id) }
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

struct ReadOnlyToastOverlay: View {
    let activeTerminalId: String?
    let onEnableEditing: () -> Void
    @State private var center = ReadOnlyToastCenter.shared

    var body: some View {
        VStack {
            if let tid = activeTerminalId, center.visibleSessionId == tid {
                HStack(spacing: 10) {
                    Image(systemName: "lock.fill")
                        .font(.system(size: 12))
                        .foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10))
                    Text("Read-only mode")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundColor(.white)
                    Text("·")
                        .foregroundColor(.white.opacity(0.3))
                    Text("Press ⌘E or click to enable editing")
                        .font(.system(size: 11))
                        .foregroundColor(.white.opacity(0.7))
                    Button("Enable") { onEnableEditing() }
                        .buttonStyle(.plain)
                        .font(.system(size: 11, weight: .semibold))
                        .padding(.horizontal, 10).padding(.vertical, 4)
                        .background(Capsule().fill(Color(red: 1.0, green: 0.78, blue: 0.10).opacity(0.2)))
                        .foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10))
                }
                .padding(.horizontal, 14).padding(.vertical, 10)
                .background(
                    RoundedRectangle(cornerRadius: 10)
                        .fill(.black.opacity(0.6))
                        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.08), lineWidth: 0.5))
                )
                .shadow(color: .black.opacity(0.3), radius: 8, y: 2)
                .transition(.opacity.combined(with: .move(edge: .top)))
                .padding(.top, 12)
            }
            Spacer()
        }
        .animation(.easeInOut(duration: 0.2), value: center.visibleSessionId)
        .allowsHitTesting(center.visibleSessionId != nil)
    }
}

struct CdFolderButton: View {
    let terminal: TerminalSession

    var body: some View {
        Button(action: pickAndCd) {
            HStack(spacing: 3) {
                Image(systemName: "folder").font(.system(size: 10))
                Text("cd").font(.system(size: 10, weight: .semibold, design: .monospaced))
            }
            .padding(.horizontal, 7).padding(.vertical, 3)
            .background(Capsule().fill(Color.white.opacity(0.06)))
            .foregroundColor(.white.opacity(0.7))
        }
        .buttonStyle(.plain)
        .help("Pick a folder and cd into it")
    }

    private func pickAndCd() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.title = "Choose a folder to cd into"
        panel.directoryURL = URL(fileURLWithPath: terminal.cwd)
        if panel.runModal() == .OK, let url = panel.url {
            let path = url.path
            // Quote to handle spaces
            let escaped = path.replacingOccurrences(of: "\"", with: "\\\"")
            terminal.terminalView.send(txt: "cd \"\(escaped)\"\n")
        }
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
    @State private var showingPopover = false
    @State private var selectedModel: ClaudeModel = .auto
    @State private var effort: EffortLevel = .none
    @State private var permissionMode: PermissionMode = .none
    @State private var dangerouslySkipPermissions = false
    @State private var resumeMode: ResumeMode = .none
    @State private var forkSession = false
    @State private var verbose = false
    @State private var name: String = ""
    @State private var addDirs: String = ""
    @State private var worktreeEnabled = false
    @State private var worktreeName: String = ""
    @State private var print: String = ""

    enum ClaudeModel: String, CaseIterable, Identifiable {
        case auto, opus, opus47, sonnet, sonnet46, haiku, haiku45
        var id: String { rawValue }
        var label: String {
            switch self {
            case .auto: return "Default"
            case .opus: return "opus"
            case .opus47: return "opus-4-7"
            case .sonnet: return "sonnet"
            case .sonnet46: return "sonnet-4-6"
            case .haiku: return "haiku"
            case .haiku45: return "haiku-4-5"
            }
        }
        var flag: String? {
            switch self {
            case .auto: return nil
            case .opus: return "--model opus"
            case .opus47: return "--model claude-opus-4-7"
            case .sonnet: return "--model sonnet"
            case .sonnet46: return "--model claude-sonnet-4-6"
            case .haiku: return "--model haiku"
            case .haiku45: return "--model claude-haiku-4-5"
            }
        }
    }

    enum EffortLevel: String, CaseIterable, Identifiable {
        case none, low, medium, high, xhigh, max
        var id: String { rawValue }
        var label: String {
            switch self {
            case .none: return "Default"
            case .low: return "low"
            case .medium: return "medium"
            case .high: return "high"
            case .xhigh: return "xhigh"
            case .max: return "max"
            }
        }
        var flag: String? {
            self == .none ? nil : "--effort \(rawValue)"
        }
    }

    enum PermissionMode: String, CaseIterable, Identifiable {
        case none, defaultMode, acceptEdits, auto, bypassPermissions, dontAsk, plan
        var id: String { rawValue }
        var label: String {
            switch self {
            case .none: return "Default"
            case .defaultMode: return "default"
            case .acceptEdits: return "acceptEdits"
            case .auto: return "auto"
            case .bypassPermissions: return "bypassPermissions"
            case .dontAsk: return "dontAsk"
            case .plan: return "plan"
            }
        }
        var flag: String? {
            switch self {
            case .none: return nil
            case .defaultMode: return "--permission-mode default"
            case .acceptEdits: return "--permission-mode acceptEdits"
            case .auto: return "--permission-mode auto"
            case .bypassPermissions: return "--permission-mode bypassPermissions"
            case .dontAsk: return "--permission-mode dontAsk"
            case .plan: return "--permission-mode plan"
            }
        }
    }

    enum ResumeMode: String, CaseIterable, Identifiable {
        case none, resume, continueLast
        var id: String { rawValue }
        var label: String {
            switch self {
            case .none: return "None"
            case .resume: return "Resume"
            case .continueLast: return "Continue"
            }
        }
        var flag: String? {
            switch self {
            case .none: return nil
            case .resume: return "--resume"
            case .continueLast: return "--continue"
            }
        }
    }

    var composedCommand: String {
        var parts = ["claude"]
        if let f = selectedModel.flag { parts.append(f) }
        if let f = effort.flag { parts.append(f) }
        if let f = permissionMode.flag { parts.append(f) }
        if dangerouslySkipPermissions { parts.append("--dangerously-skip-permissions") }
        if verbose { parts.append("--verbose") }
        if let f = resumeMode.flag { parts.append(f) }
        if forkSession && resumeMode != .none { parts.append("--fork-session") }
        if !name.trimmingCharacters(in: .whitespaces).isEmpty {
            parts.append("--name \(name.shellQuoted())")
        }
        let dirs = addDirs.split(separator: " ").map(String.init).filter { !$0.isEmpty }
        if !dirs.isEmpty {
            parts.append("--add-dir " + dirs.map { $0.shellQuoted() }.joined(separator: " "))
        }
        if worktreeEnabled {
            let trimmed = worktreeName.trimmingCharacters(in: .whitespaces)
            parts.append(trimmed.isEmpty ? "--worktree" : "--worktree \(trimmed.shellQuoted())")
        }
        if !print.trimmingCharacters(in: .whitespaces).isEmpty {
            parts.append("--print \(print.shellQuoted())")
        }
        return parts.joined(separator: " ")
    }

    var body: some View {
        Button { showingPopover = true } label: {
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
        .buttonStyle(.plain)
        .popover(isPresented: $showingPopover, arrowEdge: .top) {
            popoverContent
                .frame(width: 540)
                .frame(minHeight: 600, maxHeight: 720)
        }
    }

    private var popoverContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Image(systemName: "sparkles").foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10))
                    Text("Build a claude command").font(.system(size: 13, weight: .heavy))
                    Spacer()
                }

                Group {
                    section("MODEL") {
                        Picker("", selection: $selectedModel) {
                            ForEach(ClaudeModel.allCases) { m in Text(m.label).tag(m) }
                        }
                        .pickerStyle(.menu).labelsHidden()
                    }

                    section("EFFORT") {
                        Picker("", selection: $effort) {
                            ForEach(EffortLevel.allCases) { e in Text(e.label).tag(e) }
                        }
                        .pickerStyle(.segmented).labelsHidden()
                    }

                    section("PERMISSION MODE") {
                        Picker("", selection: $permissionMode) {
                            ForEach(PermissionMode.allCases) { m in Text(m.label).tag(m) }
                        }
                        .pickerStyle(.menu).labelsHidden()
                    }

                    section("FLAGS") {
                        Toggle(isOn: $dangerouslySkipPermissions) {
                            Text("--dangerously-skip-permissions")
                                .font(.system(size: 11, design: .monospaced))
                        }
                        Toggle(isOn: $verbose) {
                            Text("--verbose")
                                .font(.system(size: 11, design: .monospaced))
                        }
                    }

                    section("RESUME") {
                        VStack(alignment: .leading, spacing: 6) {
                            Picker("", selection: $resumeMode) {
                                ForEach(ResumeMode.allCases) { r in Text(r.label).tag(r) }
                            }
                            .pickerStyle(.segmented).labelsHidden()
                            Toggle(isOn: $forkSession) {
                                Text("--fork-session")
                                    .font(.system(size: 11, design: .monospaced))
                                    .foregroundColor(resumeMode == .none ? .secondary : .primary)
                            }
                            .disabled(resumeMode == .none)
                        }
                    }

                    section("NAME") {
                        TextField("Display name (shown in /resume picker)", text: $name)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(size: 11))
                    }

                    section("WORKTREE") {
                        VStack(alignment: .leading, spacing: 6) {
                            Toggle(isOn: $worktreeEnabled) {
                                Text("--worktree (new git worktree)")
                                    .font(.system(size: 11, design: .monospaced))
                            }
                            if worktreeEnabled {
                                TextField("Optional name", text: $worktreeName)
                                    .textFieldStyle(.roundedBorder)
                                    .font(.system(size: 11))
                            }
                        }
                    }

                    section("ADDITIONAL DIRS (--add-dir)") {
                        TextField("space-separated paths", text: $addDirs)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(size: 11, design: .monospaced))
                    }

                    section("ONE-SHOT --print") {
                        TextField("Prompt for non-interactive run", text: $print)
                            .textFieldStyle(.roundedBorder)
                            .font(.system(size: 11, design: .monospaced))
                    }
                }

                Divider()

                section("COMMAND PREVIEW") {
                    Text(composedCommand)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10))
                        .padding(.horizontal, 10).padding(.vertical, 8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color.black.opacity(0.4)))
                        .textSelection(.enabled)
                }

                HStack {
                    Button("Reset") { reset() }
                        .buttonStyle(.borderless)
                    Spacer()
                    Button("Copy") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(composedCommand, forType: .string)
                    }
                    Button("Run") {
                        terminal.sendCommand(composedCommand)
                        showingPopover = false
                    }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .tint(Color(red: 1.0, green: 0.78, blue: 0.10))
                }
            }
            .padding(20)
        }
    }

    @ViewBuilder
    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.system(size: 9, weight: .bold)).foregroundColor(.secondary).tracking(0.5)
            content()
        }
    }

    private func reset() {
        selectedModel = .auto
        effort = .none
        permissionMode = .none
        dangerouslySkipPermissions = false
        verbose = false
        resumeMode = .none
        forkSession = false
        name = ""
        addDirs = ""
        worktreeEnabled = false
        worktreeName = ""
        print = ""
    }
}

extension String {
    func shellQuoted() -> String {
        // Wrap in single quotes and escape any embedded single-quotes
        let escaped = self.replacingOccurrences(of: "'", with: "'\\''")
        return "'\(escaped)'"
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
    private var dimmed: Bool { !session.isRecentlyActive }

    private var statusColor: Color {
        if session.isAlive {
            return session.status == "busy" ? Color(red: 1.0, green: 0.78, blue: 0.10) : Color.green
        }
        return session.isRecentlyActive ? Color.green.opacity(0.5) : Color.gray.opacity(0.35)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 6) {
                Circle().fill(statusColor).frame(width: 6, height: 6)
                if isEditing {
                    TextField("Name", text: viewModel.nameInput, onCommit: { viewModel.onCommitRename(session) })
                        .textFieldStyle(.plain)
                        .scaledFont(11, weight: .semibold)
                        .foregroundColor(.white)
                } else {
                    Text(session.name)
                        .scaledFont(11, weight: .semibold)
                        .foregroundColor(dimmed ? .white.opacity(0.45) : .white.opacity(0.92))
                        .lineLimit(1)
                }
                Spacer()
                Text(fmtCost(session.cost))
                    .scaledFont(10, weight: .bold, design: .monospaced)
                    .foregroundColor(dimmed ? .orange.opacity(0.5) : .orange.opacity(0.85))
            }
            HStack(spacing: 8) {
                if let model = session.model {
                    Text(model.replacingOccurrences(of: "claude-", with: "")
                            .replacingOccurrences(of: "opus-4-7", with: "opus")
                            .replacingOccurrences(of: "opus-4.7", with: "opus")
                            .replacingOccurrences(of: "sonnet-4-6", with: "sonnet")
                            .replacingOccurrences(of: "haiku-4-5", with: "haiku"))
                        .foregroundColor(Color(red: 1.0, green: 0.78, blue: 0.10).opacity(dimmed ? 0.4 : 0.7))
                }
                Text("\(session.usage.messageCount)msg")
                if let last = session.lastActivityAt {
                    Text(relativeTime(last))
                }
                Spacer()
                Text("⚡\(fmtPct(session.cacheHitRate))")
                    .foregroundColor(session.cacheHitRate > 0.7 ? .green.opacity(0.7) : .yellow.opacity(0.7))
            }
            .scaledFont(9, design: .monospaced)
            .foregroundColor(.white.opacity(0.32))

            // Per-session token breakdown — only show if session has data
            if session.usage.messageCount > 0 {
                HStack(spacing: 6) {
                    miniToken(label: "in", n: session.usage.totalInput, color: Color(red: 1.0, green: 0.78, blue: 0.10))
                    miniToken(label: "out", n: session.usage.totalOutput, color: Color.purple.opacity(0.85))
                    miniToken(label: "cR", n: session.usage.cacheRead, color: Color.green.opacity(0.7))
                    miniToken(label: "cW", n: session.usage.cacheCreation, color: Color.orange.opacity(0.7))
                }
                .opacity(dimmed ? 0.5 : 1)
            }
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

    @ViewBuilder
    private func miniToken(label: String, n: Int, color: Color) -> some View {
        HStack(spacing: 2) {
            Text(label).font(.system(size: 8)).foregroundColor(.white.opacity(0.3))
            Text(fmtTokens(n)).font(.system(size: 9, weight: .medium, design: .monospaced)).foregroundColor(color)
        }
        .padding(.horizontal, 4).padding(.vertical, 2)
        .background(RoundedRectangle(cornerRadius: 3).fill(color.opacity(0.06)))
    }

    private func relativeTime(_ d: Date) -> String {
        let s = -d.timeIntervalSinceNow
        if s < 60 { return "now" }
        if s < 3600 { return "\(Int(s / 60))m" }
        if s < 86400 { return "\(Int(s / 3600))h" }
        return "\(Int(s / 86400))d"
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
                Toggle("Translucent terminal background", isOn: $settings.translucentBackground)
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

struct SidebarResizer: View {
    @Binding var width: CGFloat

    var body: some View {
        Rectangle()
            .fill(Color.white.opacity(0.001))
            .frame(width: 6)
            .overlay(
                Rectangle()
                    .fill(Color.white.opacity(0.04))
                    .frame(width: 1)
            )
            .onHover { hovering in
                if hovering {
                    NSCursor.resizeLeftRight.push()
                } else {
                    NSCursor.pop()
                }
            }
            .gesture(
                DragGesture()
                    .onChanged { v in
                        let newWidth = width - v.translation.width
                        width = max(220, min(500, newWidth))
                    }
            )
    }
}

/// NSVisualEffectView wrapper — gives the window the translucent vibrancy
/// effect like Terminal.app's "use background color with transparency" mode.
struct VisualEffectBackground: NSViewRepresentable {
    let material: NSVisualEffectView.Material
    let blendingMode: NSVisualEffectView.BlendingMode

    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = material
        v.blendingMode = blendingMode
        v.state = .active
        v.isEmphasized = false
        return v
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {
        view.material = material
        view.blendingMode = blendingMode
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
