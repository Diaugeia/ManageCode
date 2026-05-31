//! Embedded terminal: a PTY running a child process, with a background reader
//! thread feeding a shared `vt100` parser that the UI renders each frame via
//! `tui-term`. This replaces the old full-screen `tmux attach` handoff — the
//! terminal lives inside a ManageCode pane.

use std::io::{Read, Write};
use std::sync::{Arc, RwLock};
use std::thread;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseEvent};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use vt100::{MouseProtocolEncoding, MouseProtocolMode, Parser};

/// How many lines of scrollback the vt100 parser retains.
const SCROLLBACK: usize = 2000;

/// A live embedded terminal session.
pub struct TermSession {
    parser: Arc<RwLock<Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    rows: u16,
    cols: u16,
    /// Shown in the pane border.
    pub title: String,
}

impl TermSession {
    /// Spawn `cmd` on a fresh PTY sized `rows`x`cols`. A background thread reads
    /// the PTY and feeds the vt100 parser; the render loop only ever takes a
    /// read lock and clones the screen.
    pub fn spawn(mut cmd: CommandBuilder, rows: u16, cols: u16, title: String) -> Result<Self> {
        cmd.env("TERM", "xterm-256color");
        let rows = rows.max(1);
        let cols = cols.max(1);

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let child = pair.slave.spawn_command(cmd)?;
        // The slave handle is held by the child now; drop ours so EOF propagates
        // cleanly when the child exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let parser = Arc::new(RwLock::new(Parser::new(rows, cols, SCROLLBACK)));
        {
            let parser = Arc::clone(&parser);
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser.write() {
                                p.process(&buf[..n]);
                            }
                        }
                    }
                }
            });
        }

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            rows,
            cols,
            title,
        })
    }

    /// Resize the PTY and the parser grid in lockstep. No-op if unchanged.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.write() {
            p.screen_mut().set_size(rows, cols);
        }
    }

    /// Forward raw bytes to the child.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Translate a crossterm key into a byte sequence and forward it.
    pub fn send_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        if let Some(bytes) = encode_key(code, mods) {
            self.write_bytes(&bytes);
        }
    }

    /// Forward a mouse event (pane-relative, 1-based col/row), honoring the
    /// mouse tracking mode/encoding the child requested. No-op when the child
    /// isn't tracking the mouse.
    pub fn send_mouse(&mut self, ev: &MouseEvent, col: u16, row: u16) {
        let (mode, encoding) = match self.parser.read() {
            Ok(p) => (
                p.screen().mouse_protocol_mode(),
                p.screen().mouse_protocol_encoding(),
            ),
            Err(_) => return,
        };
        if let Some(bytes) = encode_mouse(mode, encoding, ev, col, row) {
            self.write_bytes(&bytes);
        }
    }

    /// Has the child exited?
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Clone the current screen for rendering (the documented tui-term pattern).
    pub fn screen(&self) -> vt100::Screen {
        self.parser
            .read()
            .map(|p| p.screen().clone())
            .unwrap_or_else(|_| Parser::new(self.rows, self.cols, 0).screen().clone())
    }
}

// No explicit Drop: dropping `writer` then `master` closes the PTY, which sends
// SIGHUP to the child. For a `tmux attach` child this just detaches (the tmux
// session keeps running); for a plain shell it exits. The reader thread sees
// EOF and ends on its own.

/// Map a crossterm key event to the bytes a PTY expects. Returns `None` for
/// keys we don't forward.
pub fn encode_key(code: KeyCode, mods: KeyModifiers) -> Option<Vec<u8>> {
    let bytes = match code {
        KeyCode::Char(c) => {
            if mods.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
                // Ctrl-A => 0x01 .. Ctrl-Z => 0x1a
                vec![(c.to_ascii_uppercase() as u8) - 0x40]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => return None,
        },
        _ => return None,
    };
    Some(bytes)
}

/// Encode a mouse event into the bytes the child expects, given the mouse
/// tracking `mode`/`encoding` it requested (read from the vt100 screen).
/// Returns `None` when the child isn't tracking or the event is irrelevant.
pub fn encode_mouse(
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
    ev: &MouseEvent,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    use crossterm::event::{MouseButton, MouseEventKind as K};

    if matches!(mode, MouseProtocolMode::None) {
        return None;
    }

    let btn = |b: MouseButton| -> u16 {
        match b {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    };

    // (button code, is_press) — press selects the SGR final byte 'M' vs 'm'.
    let (mut code, press) = match ev.kind {
        K::Down(b) => (btn(b), true),
        K::Up(b) => {
            if matches!(mode, MouseProtocolMode::Press) {
                return None;
            }
            (btn(b), false)
        }
        K::Drag(b) => {
            if !matches!(
                mode,
                MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
            ) {
                return None;
            }
            (btn(b) + 32, true)
        }
        K::Moved => {
            if !matches!(mode, MouseProtocolMode::AnyMotion) {
                return None;
            }
            (3 + 32, true)
        }
        K::ScrollUp => (64, true),
        K::ScrollDown => (65, true),
        K::ScrollLeft => (66, true),
        K::ScrollRight => (67, true),
    };

    if ev.modifiers.contains(KeyModifiers::SHIFT) {
        code += 4;
    }
    if ev.modifiers.contains(KeyModifiers::ALT) {
        code += 8;
    }
    if ev.modifiers.contains(KeyModifiers::CONTROL) {
        code += 16;
    }

    match encoding {
        MouseProtocolEncoding::Sgr => {
            let f = if press { 'M' } else { 'm' };
            Some(format!("\x1b[<{};{};{}{}", code, col, row, f).into_bytes())
        }
        // Legacy X10/UTF-8: press-only, every field offset by 32.
        _ => {
            let cb = if press { code } else { 3 };
            Some(vec![
                0x1b,
                b'[',
                b'M',
                (cb + 32).min(255) as u8,
                (col + 32).min(255) as u8,
                (row + 32).min(255) as u8,
            ])
        }
    }
}

/// What to launch in a freshly-opened embedded terminal. `argv` empty means the
/// user's login shell.
#[derive(Clone)]
pub struct TerminalSpec {
    pub cwd: String,
    pub argv: Vec<String>,
    pub title: String,
}

impl TerminalSpec {
    pub fn build_command(&self) -> CommandBuilder {
        let mut cmd = if self.argv.is_empty() {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let mut c = CommandBuilder::new(shell);
            c.arg("-l");
            c
        } else {
            let mut c = CommandBuilder::new(&self.argv[0]);
            for a in &self.argv[1..] {
                c.arg(a);
            }
            c
        };
        cmd.cwd(&self.cwd);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn captures_child_output() {
        let spec = TerminalSpec {
            cwd: ".".into(),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'hello_pty_42'".into(),
            ],
            title: "t".into(),
        };
        let t = TermSession::spawn(spec.build_command(), 24, 80, "t".into()).unwrap();
        // Let the background reader drain the PTY and feed the parser.
        std::thread::sleep(Duration::from_millis(400));
        let contents = t.screen().contents();
        assert!(
            contents.contains("hello_pty_42"),
            "screen did not capture child output: {contents:?}"
        );
    }

    #[test]
    fn encodes_keys() {
        assert_eq!(
            encode_key(KeyCode::Char('a'), KeyModifiers::CONTROL),
            Some(vec![0x01])
        );
        assert_eq!(
            encode_key(KeyCode::Char('x'), KeyModifiers::NONE),
            Some(vec![b'x'])
        );
        assert_eq!(encode_key(KeyCode::Enter, KeyModifiers::NONE), Some(vec![b'\r']));
        assert_eq!(encode_key(KeyCode::Up, KeyModifiers::NONE), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key(KeyCode::Esc, KeyModifiers::NONE), Some(vec![0x1b]));
    }
}
