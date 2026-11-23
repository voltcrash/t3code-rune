use super::{Desktop, Labels, Panel, Tab, button};
use crate::{Request, workspace::label};
use rune::{El, FileDialogOptions, col, el, row, tok};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct File {
    id: String,
    file_name: String,
}

fn save(
    s: &Desktop,
    p: &Panel,
    file: &File,
    caption: &str,
    primary: bool,
    theme: &tok::Theme,
) -> El {
    let (watch, operation) = (p.watch, p.operation);
    let tab = p.page.selected.clone().unwrap_or_default();
    let id = file.id.clone();
    let filename = crate::attachments::suggested_filename(
        &file.file_name,
        &s.workspace.attachment_labels.file,
    );
    let title = s
        .workspace
        .attachment_labels
        .download
        .replace("{label}", &filename);
    let dialog_title = title.clone();
    let button = button(
        &title,
        label(
            caption,
            12.,
            500.,
            if primary { theme.bg } else { theme.text },
        ),
        true,
        theme,
        move |_, tx| {
            let (tab, download) = (tab.clone(), id.clone());
            tx.save_file(
                FileDialogOptions {
                    title: dialog_title.clone(),
                    file_name: Some(filename.clone()),
                    ..Default::default()
                },
                move |s: &mut Desktop, result, _| match result {
                    Ok(Some(path)) => {
                        s.write_request(
                            None,
                            Request::DownloadBrowserFile {
                                watch,
                                operation,
                                tab,
                                download,
                                path,
                            },
                        );
                    }
                    Ok(None) => {}
                    Err(error) => s.error = Some(error.to_string()),
                },
            );
        },
    );
    if primary {
        button
            .h(30.)
            .px(10.)
            .fill(theme.text)
            .hover(theme.text.alpha(0.85))
    } else {
        button
    }
}

pub(super) fn notice(
    s: &Desktop,
    p: &Panel,
    file: &File,
    labels: &Labels,
    theme: &tok::Theme,
) -> El {
    let tab = p.page.selected.clone();
    let id = file.id.clone();
    row()
        .w_full()
        .px(8.)
        .gap(4.)
        .center_y()
        .child(
            label(
                &labels.downloaded.replace("{fileName}", &file.file_name),
                12.,
                500.,
                theme.text,
            )
            .role("status")
            .min_w(0.)
            .grow(1.)
            .truncate(),
        )
        .child(save(s, p, file, &labels.save, false, theme))
        .child(button(
            &labels.dismiss,
            rune::icon(&rune_kit::glyphs::CLOSE, 14.).color(theme.text_secondary),
            true,
            theme,
            move |s, _| {
                s.browser_request(json!({"action":"dismissDownload","tab":tab,"id":id}));
            },
        ))
}

pub(super) fn file_page(
    s: &Desktop,
    p: &Panel,
    tab: &Tab,
    file: &File,
    labels: &Labels,
    theme: &tok::Theme,
) -> El {
    let url = url::Url::parse(&tab.url).ok();
    let host = url
        .as_ref()
        .map(|u| &u[url::Position::BeforeHost..url::Position::AfterPort])
        .unwrap_or(&tab.url);
    let description = labels
        .download_source
        .replace("{host}", host)
        .replace("{fileName}", &file.file_name);
    let mut actions = row().w_full().gap(8.).child(el().grow(1.));
    if let Some(url) = url
        && matches!(url.scheme(), "http" | "https")
    {
        let destination = external_url(&url);
        actions = actions.child(
            button(
                &labels.open_external,
                label(&labels.open_external, 12., 500., theme.text),
                true,
                theme,
                move |s, tx| {
                    if let Err(error) = tx.open_url(&destination) {
                        s.error = Some(error.to_string());
                    }
                },
            )
            .h(30.)
            .px(10.)
            .border(1., theme.card_border),
        );
    }
    col()
        .key("browser-file-download")
        .w_full()
        .max_w(576.)
        .self_center()
        .grow(1.)
        .min_h(0.)
        .px(32.)
        .py(48.)
        .gap(10.)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .child(rune::icon(&FILE_UNAVAILABLE, 48.).color(theme.text_secondary.alpha(0.7)))
        .child(el().h(4.))
        .child(label(&labels.file_not_shown, 24., 600., theme.text).wrap())
        .child(label(&description, 14., 400., theme.text_secondary).wrap())
        .child(el().grow(1.))
        .child(actions.child(save(s, p, file, &labels.file_download, true, theme)))
}

static FILE_UNAVAILABLE: rune::Icon = rune::Icon {
    name: "browser-file-unavailable",
    view: 64.,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="2.5"><path d="M16 12 L48 12 L48 52 L16 52 Z"/><path d="M22 22 L42 22 M22 30 L36 30 M22 38 L40 38 M52 8 L12 56" stroke-linecap="round"/></g>"#,
};

fn external_url(url: &url::Url) -> String {
    use url::Position::BeforePath;
    let mut destination = url[..BeforePath].to_owned();
    for c in url[BeforePath..].chars() {
        match c {
            '[' => destination.push_str("%5B"),
            ']' => destination.push_str("%5D"),
            '^' => destination.push_str("%5E"),
            '{' => destination.push_str("%7B"),
            '|' => destination.push_str("%7C"),
            '}' => destination.push_str("%7D"),
            '`' => destination.push_str("%60"),
            '\\' => destination.push_str("%5C"),
            _ => destination.push(c),
        }
    }
    destination
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_downloads_with_query_brackets_and_an_ipv6_host() {
        let url = url::Url::parse("https://[::1]/report.csv?columns[]=name&sort=%2B").unwrap();
        assert_eq!(
            external_url(&url),
            "https://[::1]/report.csv?columns%5B%5D=name&sort=%2B"
        );
    }
}
