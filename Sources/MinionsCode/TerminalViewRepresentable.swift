import AppKit
import SwiftUI
import SwiftTerm

struct TerminalViewRepresentable: NSViewRepresentable {
    let terminalView: LocalProcessTerminalView

    func makeNSView(context: Context) -> NSView {
        terminalView.translatesAutoresizingMaskIntoConstraints = false
        terminalView.disableFullRedrawOnAnyChanges = true

        let container = TerminalContainer()
        container.addSubview(terminalView)
        NSLayoutConstraint.activate([
            terminalView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            terminalView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            terminalView.topAnchor.constraint(equalTo: container.topAnchor),
            terminalView.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
        container.terminal = terminalView
        return container
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

class TerminalContainer: NSView {
    var terminal: LocalProcessTerminalView?

    override var acceptsFirstResponder: Bool { true }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in
            self?.window?.makeFirstResponder(self?.terminal)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(terminal)
        terminal?.mouseDown(with: event)
    }
}
