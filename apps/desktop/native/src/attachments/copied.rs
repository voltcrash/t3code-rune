use super::{FILE_LIMIT, Selection, Target};
use crate::Desktop;
use rune::{CopyDestination, TextEdit};
use std::{cell::RefCell, rc::Rc};

#[derive(Default, Clone)]
pub(crate) struct Clipboard(Rc<RefCell<[Option<Copied>; 2]>>);

struct Copied {
    text: String,
    files: Vec<Selection>,
}

impl Clipboard {
    pub(super) fn copy(
        &self,
        edit: &TextEdit,
        dest: CopyDestination,
        files: &[Selection],
    ) -> Option<String> {
        let text = crate::mentions::copy_selection(edit);
        let (start, end) = edit.range();
        let mut slots = self.0.borrow_mut();
        slots[match dest {
            CopyDestination::Clipboard => 0,
            CopyDestination::PrimarySelection => 1,
        }] = text.as_ref().and_then(|text| {
            let selected: Vec<_> = files
                .iter()
                .filter(|file| {
                    file.pasted.as_ref().is_some_and(|paste| {
                        edit.pills()
                            .any(|(r, id)| r.start >= start && r.end <= end && id == paste.pill)
                    })
                })
                .cloned()
                .collect();
            (!selected.is_empty()).then(|| Copied {
                text: text.clone(),
                files: selected,
            })
        });
        text
    }

    fn files(&self, text: &str) -> Option<Vec<Selection>> {
        self.0
            .borrow()
            .iter()
            .flatten()
            .find(|clip| clip.text == text)
            .map(|clip| clip.files.clone())
    }
}

pub(crate) fn release_unreachable(files: &mut Vec<Selection>, edit: &TextEdit) {
    if files.iter().all(|file| file.selected(edit)) {
        return;
    }
    let retained: std::collections::HashSet<_> = edit.retained_pill_ids().collect();
    files.retain(|file| {
        file.pasted
            .as_ref()
            .is_none_or(|paste| retained.contains(paste.pill.as_str()))
    });
}

impl Target {
    pub(super) fn paste_copied(&self, app: &mut Desktop, text: &str) -> bool {
        let Some(copied) = app.copied_files.files(text) else {
            return false;
        };
        let limits = app.text_paste;
        let remove_some = app.workspace.attachment_labels.remove_some.clone();
        let too_large = app.workspace.attachment_labels.too_large.clone();
        let mut error = None;
        self.edit(app, |files, edit| {
            if super::retained_input_units(edit) + text.encode_utf16().count() > limits.input {
                error = Some(remove_some);
                return;
            }
            if copied.iter().any(|file| {
                file.pasted
                    .as_ref()
                    .is_some_and(|paste| paste.bytes > limits.file)
            }) {
                error = Some(too_large);
                return;
            }
            let incoming: Vec<_> = copied
                .iter()
                .filter(|file| !files.iter().any(|old| old.path == file.path))
                .cloned()
                .collect();
            if files.len() + incoming.len() > FILE_LIMIT {
                error = Some(super::TOO_MANY.into());
                return;
            }
            let mut links: Vec<_> = copied
                .iter()
                .filter_map(|file| file.pasted.as_ref())
                .flat_map(|paste| {
                    text.match_indices(&paste.link)
                        .map(move |(at, link)| (at, at + link.len(), paste))
                })
                .collect();
            links.sort_unstable_by_key(|(from, _, _)| *from);
            edit.undo_group(|edit| {
                let mut at = 0;
                for (start, end, paste) in links {
                    if at < start {
                        edit.insert(&text[at..start]);
                    }
                    edit.insert_pill(&paste.pill, &paste.name);
                    at = end;
                }
                if at < text.len() {
                    edit.insert(&text[at..]);
                }
            });
            files.extend(incoming);
        });
        if let Some(error) = error {
            app.error = Some(error);
        }
        true
    }
}
