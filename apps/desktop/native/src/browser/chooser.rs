use super::*;
use rune::FileDialogOptions;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Labels {
    title: String,
    asks_files: String,
    asks_file: String,
    cancel: String,
    choose_files: String,
    choose_file: String,
}

#[derive(Deserialize)]
pub(super) struct Chooser {
    id: u64,
    multiple: bool,
    accept: String,
}

/// The page's extension tokens become a dialog filter; a MIME token keeps every file selectable.
fn filters(accept: &str) -> Vec<(String, Vec<String>)> {
    let extensions: Option<Vec<String>> = accept
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.strip_prefix('.').map(str::to_ascii_lowercase))
        .collect();
    match extensions {
        Some(extensions) if !extensions.is_empty() => vec![(accept.to_owned(), extensions)],
        _ => Vec::new(),
    }
}

impl Desktop {
    fn answer_file_chooser(&mut self, id: u64, paths: Vec<std::path::PathBuf>) {
        let Some(p) = &self.workspace.browser.panel else {
            return;
        };
        if p.page.controlled && p.page.file_chooser.as_ref().is_some_and(|c| c.id == id) {
            self.browser_request(
                json!({"action":"fileChooser", "tab":p.page.selected, "id":id, "paths":paths}),
            );
        }
    }
}

pub(super) fn view(c: &Chooser, labels: &Labels, theme: &tok::Theme) -> El {
    let id = c.id;
    let label = |text: &str, color| super::super::workspace::label(text, 12., 500., color);
    let (prompt, choose) = if c.multiple {
        (&labels.asks_files, &labels.choose_files)
    } else {
        (&labels.asks_file, &labels.choose_file)
    };
    let title = choose.clone();
    let filters = filters(&c.accept);
    let card = col()
        .role("dialog")
        .label(&labels.title)
        .w_full()
        .px(12.)
        .py(12.)
        .gap(8.)
        .radius(8.)
        .border(1., theme.separator)
        .fill(theme.bg)
        .child(super::super::workspace::label(prompt, 13., 400., theme.text).wrap())
        .child(
            row()
                .w_full()
                .gap(6.)
                .child(rune::spacer())
                .child(
                    button(
                        &labels.cancel,
                        label(&labels.cancel, theme.text),
                        true,
                        theme,
                        move |s, _| s.answer_file_chooser(id, Vec::new()),
                    )
                    .key("browser-file-chooser-cancel")
                    .h(30.)
                    .px(10.)
                    .border(1., theme.card_border),
                )
                .child(
                    button(
                        choose,
                        label(choose, theme.bg),
                        true,
                        theme,
                        move |_, tx| {
                            let options = FileDialogOptions {
                                title: title.clone(),
                                filters: filters.clone(),
                                ..Default::default()
                            };
                            tx.open_file(options, move |s: &mut Desktop, result, _| match result {
                                Ok(Some(path)) => s.answer_file_chooser(id, vec![path]),
                                Ok(None) => {}
                                Err(error) => s.error = Some(error.to_string()),
                            });
                        },
                    )
                    .key("browser-file-chooser-choose")
                    .h(30.)
                    .px(10.)
                    .fill(theme.text)
                    .hover(theme.text.alpha(0.85)),
                ),
        );
    col().absolute(0., 8.).w_full().px(8.).child(card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek};

    #[test]
    fn filters_only_extension_lists() {
        assert_eq!(
            filters(".PNG, .jpg"),
            vec![(
                ".PNG, .jpg".to_owned(),
                vec!["png".to_owned(), "jpg".to_owned()]
            )]
        );
        assert!(filters("image/*,.pdf").is_empty());
        assert!(filters("").is_empty());
    }

    #[test]
    fn answers_the_open_chooser_with_the_picked_file_or_a_cancel() {
        let mut wire = tempfile::tempfile().unwrap();
        #[cfg(unix)]
        let output = std::os::fd::OwnedFd::from(wire.try_clone().unwrap());
        #[cfg(windows)]
        let output = std::os::windows::io::OwnedHandle::from(wire.try_clone().unwrap());
        let size = (320., 180.);
        let spec = rune::AppSpec::new("Browser", size, Desktop::new, |s: &Desktop, _| {
            let labels: Labels = serde_json::from_value(json!({
                "title": "Choose files for the page", "asksFiles": "The page asks for files.",
                "asksFile": "The page asks for a file.", "cancel": "Cancel",
                "chooseFiles": "Choose files", "chooseFile": "Choose file",
            }))
            .unwrap();
            let chooser = s
                .workspace
                .browser
                .panel
                .as_ref()
                .and_then(|p| p.page.file_chooser.as_ref());
            col()
                .w_full()
                .h_full()
                .children(chooser.map(|c| view(c, &labels, &tok::theme(false))))
        });
        let mut rt = rune::App::new(spec).headless_runtime(size, Default::default());
        rt.update(move |s, _| {
            s.output = Some(output.into());
            let mut panel = Panel::new(2, 55);
            panel.page.selected = Some("a".into());
            panel.page.controlled = true;
            panel.page.file_chooser = Some(Chooser {
                id: 7,
                multiple: false,
                accept: ".csv".into(),
            });
            s.workspace.browser.panel = Some(panel);
        });
        rt.frame(0.);
        let (x, y) = rt.find("browser-file-chooser-choose").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        let (_, options) = rt.file_dialog().unwrap();
        assert_eq!(options.title, "Choose file");
        assert_eq!(
            options.filters,
            vec![(".csv".to_owned(), vec!["csv".to_owned()])]
        );
        rt.resolve_file_dialog(Some("/tmp/report.csv".into()))
            .unwrap();
        rt.frame(0.);
        let (x, y) = rt.find("browser-file-chooser-cancel").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.);
        rt.update(|s, _| s.workspace.browser.panel.as_mut().unwrap().page.controlled = false);
        rt.frame(0.);
        rt.update(|s, _| s.answer_file_chooser(7, Vec::new()));
        wire.rewind().unwrap();
        let mut text = String::new();
        wire.read_to_string(&mut text).unwrap();
        let sent: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let request = |paths: Value| json!({"type":"browser","watch":2,"operation":55,"action":"fileChooser","tab":"a","id":7,"paths":paths});
        assert_eq!(
            sent,
            [request(json!(["/tmp/report.csv"])), request(json!([]))]
        );
    }
}
