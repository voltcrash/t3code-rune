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

fn save(s: &Desktop, p: &Panel, file: &File, labels: &Labels, theme: &tok::Theme) -> El {
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
    button(
        &title,
        label(&labels.save, 12., 500., theme.text),
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
    )
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
        .child(save(s, p, file, labels, theme))
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
    let mut actions = row().w_full().gap(8.).child(el().grow(1.));
    if let Ok(url) = url::Url::parse(&tab.url)
        && matches!(url.scheme(), "http" | "https")
    {
        actions = actions.child(button(
            &labels.open_external,
            label(&labels.open_external, 12., 500., theme.text),
            true,
            theme,
            move |_, tx| {
                let _ = tx.open_url(url.as_str());
            },
        ));
    }
    col()
        .key("browser-file-download")
        .w_full()
        .grow(1.)
        .min_h(0.)
        .px(24.)
        .py(20.)
        .gap(10.)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .child(rune::icon(&rune_kit::glyphs::FILE, 36.).color(theme.text_secondary))
        .child(label(&labels.file_not_shown, 20., 600., theme.text).wrap())
        .child(
            label(
                &labels.downloaded.replace("{fileName}", &file.file_name),
                13.,
                400.,
                theme.text_secondary,
            )
            .wrap(),
        )
        .child(el().grow(1.))
        .child(actions.child(save(s, p, file, labels, theme)))
}
