use super::*;

#[derive(Default)]
pub(super) struct Input {
    focused: bool,
    caret: [f32; 2],
    preedit: String,
}

impl Input {
    pub(super) fn reset(&mut self) {
        self.focused = false;
        self.preedit.clear();
    }
}

impl Desktop {
    fn browser_mouse(
        &mut self,
        [x, y]: [f32; 2],
        action: &str,
        button: &str,
        buttons: u8,
        clicks: u32,
        modifiers: rune::Mods,
    ) {
        self.browser_input(json!({
            "type": "mouse", "action": action, "x": x, "y": y,
            "button": button, "buttons": buttons, "clickCount": clicks, "modifiers": mods(modifiers)
        }));
    }

    pub(super) fn sync_browser_input(&mut self, tx: &mut Tx) -> bool {
        let Some(p) = &mut self.workspace.browser.panel else {
            return false;
        };
        if std::mem::take(&mut p.focus_dialog)
            && tx.focused() == Some("browser-page")
            && p.page.controlled
            && let Some(dialog) = &p.page.dialog
        {
            tx.focus_when_shown(
                if dialog.prompt() {
                    "browser-response"
                } else {
                    "browser-dismiss"
                },
                true,
            );
        }
        let focused =
            tx.focused() == Some("browser-page") && p.page.controlled && p.page.dialog.is_none();
        if p.input.focused == focused {
            return false;
        }
        p.input.focused = focused;
        p.input.preedit.clear();
        true
    }

    pub(crate) fn browser_ime(&mut self, ev: &rune::Ime, tx: &mut Tx) -> bool {
        let Some(p) = &mut self.workspace.browser.panel else {
            return false;
        };
        if tx.focused() != Some("browser-page") || !p.page.controlled || p.page.dialog.is_some() {
            return false;
        }
        match ev {
            rune::Ime::Preedit(text, ..) => p.input.preedit.clone_from(text),
            rune::Ime::Commit(text) => {
                p.input.preedit.clear();
                if !text.is_empty() {
                    self.browser_input(json!({"type":"text","text":text}));
                }
            }
            rune::Ime::Disabled => p.input.preedit.clear(),
            _ => {}
        }
        true
    }
}

fn key(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx, down: bool) -> bool {
    if !s
        .workspace
        .browser
        .panel
        .as_ref()
        .is_some_and(|p| p.page.controlled)
    {
        return false;
    }
    if ev.key == Key::Escape && ev.mods.shift {
        if down {
            tx.focus("browser-address", true);
        }
        return true;
    }
    if ev.mods.shift && ev.is_command('j') {
        return false;
    }
    let (key, code_num) = match &ev.key {
        Key::Text(v) => (
            v.clone(),
            v.chars()
                .next()
                .filter(char::is_ascii)
                .map_or(0, |c| c.to_ascii_uppercase() as u32),
        ),
        Key::Command(c) => (c.to_string(), c.to_ascii_uppercase() as u32),
        Key::Left => ("ArrowLeft".into(), 37),
        Key::Up => ("ArrowUp".into(), 38),
        Key::Right => ("ArrowRight".into(), 39),
        Key::Down => ("ArrowDown".into(), 40),
        Key::Enter => ("Enter".into(), 13),
        Key::Tab => ("Tab".into(), 9),
        Key::Backspace => ("Backspace".into(), 8),
        Key::Delete => ("Delete".into(), 46),
        Key::Escape => ("Escape".into(), 27),
        Key::Home => ("Home".into(), 36),
        Key::End => ("End".into(), 35),
        Key::PageUp => ("PageUp".into(), 33),
        Key::PageDown => ("PageDown".into(), 34),
        _ => return false,
    };
    let modifiers = mods(ev.mods);
    let text = (down && modifiers & 7 == 0).then(|| {
        if ev.key == Key::Enter {
            "\r"
        } else if matches!(ev.key, Key::Text(_)) {
            &key
        } else {
            ""
        }
    });
    let code = tx
        .physical_key()
        .map_or_else(|| key.clone(), |k| format!("{k:?}"));
    let mut value = json!({"type":"key","action":if down {"down"} else {"up"},"key":key,"code":code,"keyCode":code_num,"modifiers":modifiers});
    if let Some(t) = text.filter(|t| !t.is_empty()) {
        value["text"] = json!(t)
    }
    s.browser_input(value);
    true
}
fn mods(m: rune::Mods) -> u8 {
    u8::from(m.alt) | (u8::from(m.ctrl) << 1) | (u8::from(m.logo) << 2) | (u8::from(m.shift) << 3)
}

pub(super) fn surface(mut frame: El, panel: &Panel, theme: &tok::Theme) -> El {
    if panel.input.focused {
        let [x, y] = panel.input.caret;
        let mark = el().size(1., 16.).absolute(x, y).ime_cursor();
        frame = frame.child(mark);
        if !panel.input.preedit.is_empty() {
            frame = frame.child(
                super::super::workspace::label(&panel.input.preedit, 13., 400., theme.text)
                    .fill(theme.bg)
                    .px(2.)
                    .absolute(x, y),
            );
        }
    }
    frame
        .focusable(true)
        .pointer_buttons([rune::PointerButton::Middle])
        .on_secondary_press_local(|s: &mut Desktop, e, tx| {
            if let Some(point) = s.browser_coords(e.x, e.y) {
                tx.focus("browser-page", false);
                s.browser_mouse(point, "down", "right", 2, 1, tx.mods());
                s.browser_mouse(point, "up", "right", 0, 1, tx.mods());
            }
        })
        .on_pointer_move_local(|s: &mut Desktop, e, tx| {
            if s.workspace
                .browser
                .panel
                .as_ref()
                .is_some_and(|p| !p.dragging)
                && let Some(point) = s.browser_coords(e.x, e.y)
            {
                s.browser_mouse(point, "move", "none", 0, 0, tx.mods());
            }
            tx.live_only();
        })
        .on_drag(|s: &mut Desktop, d, tx| {
            let Some(point) = s.browser_coords(d.x, d.y) else {
                return;
            };
            let Some(panel) = &mut s.workspace.browser.panel else {
                return;
            };
            let (button, mask) = match tx.button() {
                rune::PointerButton::Middle => ("middle", 4),
                _ => ("left", 1),
            };
            let (action, buttons) = match d.phase {
                rune::Phase::Down => {
                    tx.focus("browser-page", false);
                    panel.input.caret = [d.x, d.y];
                    panel.dragging = true;
                    ("down", mask)
                }
                rune::Phase::Move if panel.dragging => ("move", mask),
                rune::Phase::Up if panel.dragging => {
                    panel.dragging = false;
                    ("up", 0)
                }
                _ => return,
            };
            s.browser_mouse(point, action, button, buttons, d.clicks, tx.mods());
        })
        .on_wheel(|s: &mut Desktop, e, _| {
            let Some([x, y]) = s.browser_coords(e.x, e.y) else {
                return false;
            };
            s.browser_input(json!({
                "type": "wheel", "x": x, "y": y,
                "deltaX": -e.dx, "deltaY": -e.dy, "modifiers": mods(e.mods)
            }));
            true
        })
        .on_key(|s: &mut Desktop, e, tx| key(s, e, tx, true))
        .on_key_up(|s: &mut Desktop, e, tx| key(s, e, tx, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek};

    #[test]
    fn only_the_controlled_focused_page_receives_the_composition_commit() {
        let mut wire = tempfile::tempfile().unwrap();
        #[cfg(unix)]
        let output = std::os::fd::OwnedFd::from(wire.try_clone().unwrap());
        #[cfg(windows)]
        let output = std::os::windows::io::OwnedHandle::from(wire.try_clone().unwrap());
        let size = (320., 180.);
        let spec = rune::AppSpec::new("Browser", size, Desktop::new, |_, _| {
            row()
                .child(
                    el().key("browser-page")
                        .focusable(true)
                        .size(90., 36.)
                        .ime_cursor(),
                )
                .child(el().key("toolbar").focusable(true).size(90., 36.))
        });
        let mut rt = rune::App::new(spec)
            .on_ime(Desktop::browser_ime)
            .headless_runtime(size, Default::default());
        rt.update(move |s, tx| {
            s.output = Some(output.into());
            let mut panel = Panel::new(2, 55);
            panel.page.selected = Some("a".into());
            panel.page.controlled = true;
            s.workspace.browser.panel = Some(panel);
            tx.focus("browser-page", false);
        });
        rt.frame(0.);
        rt.ime(rune::Ime::Preedit("にほん".into(), None, Vec::new()));
        rt.frame(0.);
        rt.ime(rune::Ime::Commit("日本".into()));
        rt.frame(0.);
        rt.update(|_, tx| tx.focus("toolbar", false));
        rt.frame(0.);
        rt.ime(rune::Ime::Commit("other control".into()));
        rt.frame(0.);
        rt.update(|s, tx| {
            s.workspace.browser.panel.as_mut().unwrap().page.controlled = false;
            tx.focus("browser-page", false);
        });
        rt.frame(0.);
        rt.ime(rune::Ime::Commit("read only".into()));
        rt.frame(0.);
        wire.rewind().unwrap();
        let mut text = String::new();
        wire.read_to_string(&mut text).unwrap();
        let sent: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(sent["input"], json!({"type":"text","text":"日本"}));
        assert_eq!(sent["tab"], "a");
    }
}
