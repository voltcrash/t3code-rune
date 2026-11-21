use crate::Desktop;
use rune::{App, ElementInfo, Key, KeyEvent};
use std::{cell::Cell, io, rc::Rc};

pub(super) struct LaunchCheck {
    thread: String,
    title: String,
    message: String,
}

impl LaunchCheck {
    pub(super) fn requested() -> io::Result<Option<Self>> {
        let mut args = std::env::args_os().skip(1);
        if args.next().as_deref() != Some("--verify-launch".as_ref()) {
            return Ok(None);
        }
        let mut next = || {
            args.next()
                .and_then(|s| s.into_string().ok())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| io::Error::other("Expected a fixture thread ID, title and message"))
        };
        let check = Self {
            thread: next()?,
            title: next()?,
            message: next()?,
        };
        if !std::env::var_os("T3CODE_HOME")
            .is_some_and(|home| std::path::Path::new(&home).is_absolute())
        {
            return Err(io::Error::other(
                "Set T3CODE_HOME to the isolated fixture directory",
            ));
        }
        Ok(Some(check))
    }

    pub(super) fn observe(self, app: App<Desktop>, passed: Rc<Cell<bool>>) -> App<Desktop> {
        let handle = app.handle();
        let caption = format!("{}, ", self.title);
        let mut requested_focus = false;
        let mut opened = false;
        app.on_frame_drawn(move |runtime| {
            let nodes = runtime.elements();
            let visible = |node: &&ElementInfo| {
                node.visible[2] > 0. && node.visible[3] > 0. && node.opacity > 0. && !node.exiting
            };
            if opened {
                if runtime
                    .state
                    .workspace
                    .visible_thread()
                    .is_some_and(|t| t.id == self.thread)
                    && nodes
                        .iter()
                        .filter(visible)
                        .any(|node| node.text.as_deref() == Some(&self.message))
                {
                    passed.set(true);
                    handle.close();
                }
            } else if let Some(button) = nodes.iter().filter(visible).find(|node| {
                node.focusable
                    && !node.inert
                    && node.role == "button"
                    && node.key.as_ref() == Some(&self.thread)
                    && node
                        .label
                        .as_deref()
                        .is_some_and(|label| label == self.title || label.starts_with(&caption))
            }) {
                if button.focused {
                    eprintln!("Rune launch check: opening the focused sidebar row");
                    opened = true;
                    handle.update(|_, tx| {
                        tx.send_key(KeyEvent {
                            key: Key::Text(" ".into()),
                            mods: Default::default(),
                        })
                    });
                } else if !requested_focus {
                    eprintln!("Rune launch check: found the seeded sidebar row");
                    requested_focus = true;
                    let key = self.thread.clone();
                    handle.update(move |_, tx| tx.focus_when_shown(key, true));
                }
            }
        })
    }
}
