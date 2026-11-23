use super::{Desktop, SettingsLabels, SettingsPage, label};
use rune::{El, Key, KeyEvent, TextStyle, Tx, col, el, icon, row, text_input, tok};

/// What the nav search can find: each page's title and the settings it shows.
fn index(labels: &SettingsLabels) -> Vec<(SettingsPage, &str)> {
    vec![
        (SettingsPage::Appearance, labels.appearance.as_str()),
        (SettingsPage::Appearance, "Color scheme"),
        (SettingsPage::Appearance, "Contrast"),
        (SettingsPage::Appearance, "Chat width"),
        (SettingsPage::Appearance, "Interface font"),
        (SettingsPage::Appearance, "Monospace font"),
        (SettingsPage::Chat, labels.chat.as_str()),
        (SettingsPage::Chat, labels.slash_skills.as_str()),
        (SettingsPage::Chat, labels.window_title.as_str()),
        (SettingsPage::Voice, labels.voice.title.as_str()),
        (SettingsPage::Voice, labels.voice.microphone_title()),
        (SettingsPage::Voice, labels.voice.models.as_str()),
        (SettingsPage::Archive, labels.archive.title.as_str()),
    ]
}

fn matches<'a>(labels: &'a SettingsLabels, query: &str) -> Vec<(SettingsPage, &'a str)> {
    // Like the web search, every word must appear in the setting or its page's name.
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    index(labels)
        .into_iter()
        .filter(|(page, title)| {
            let text = format!("{title} {}", page.title(Some(labels))).to_lowercase();
            words.iter().all(|word| text.contains(word))
        })
        .collect()
}

impl Desktop {
    pub(super) fn clear_settings_search(&mut self) -> bool {
        if self.settings_search.text.is_empty() {
            return false;
        }
        self.settings_search = rune::TextEdit::new("");
        true
    }

    /// "/" jumps to the search field, as on the web, unless a text field has focus.
    pub(crate) fn settings_search_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if self.settings_labels.is_some()
            && matches!(&ev.key, Key::Text(t) if t == "/")
            && !ev.mods.cmd
            && !ev.mods.ctrl
        {
            tx.focus("settings-search", true);
            return true;
        }
        false
    }
}

fn search_key(s: &mut Desktop, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.key == Key::Escape && s.clear_settings_search() {
        tx.focus("settings-search", true);
        return true;
    }
    false
}

pub(super) fn field(s: &Desktop, labels: &SettingsLabels, theme: &tok::Theme) -> El {
    let edit = text_input(&s.settings_search, TextStyle::new(14., 500.))
        .key("settings-search")
        .label(&labels.search_label)
        .min_w(0.)
        .grow(1.)
        .h(21.)
        .color(theme.text)
        .placeholder(&labels.search, theme.text_secondary)
        .single_line()
        .on_edit(|s: &mut Desktop| &mut s.settings_search)
        .on_input_key(search_key);
    let mut field = row()
        .w_full()
        .h(32.)
        .px(8.)
        .gap(8.)
        .center_y()
        .radius(6.)
        .hover(theme.text.alpha(0.03))
        .child(icon(&rune_kit::glyphs::SEARCH, 16.).color(theme.text_secondary))
        .child(edit);
    field = if s.settings_search.text.is_empty() {
        field.child(
            el().size(20., 20.)
                .radius(4.)
                .center()
                .fill(theme.text.alpha(0.07))
                .a11y_hidden()
                .child(label("/", 12., 500., theme.text_secondary)),
        )
    } else {
        field.child(
            el().key("settings-search-clear")
                .role("button")
                .label(&labels.clear_search)
                .focusable(true)
                .size(20., 20.)
                .radius(4.)
                .center()
                .hover(theme.text.alpha(0.08))
                .child(icon(&rune_kit::glyphs::CLOSE, 12.).color(theme.text_secondary))
                .on_press_tx(|s: &mut Desktop, tx| {
                    s.clear_settings_search();
                    tx.focus("settings-search", true);
                }),
        )
    };
    field
}

/// The nav while a query is typed: matching settings with the page they live on.
pub(super) fn results(s: &Desktop, labels: &SettingsLabels, theme: &tok::Theme) -> Option<El> {
    if s.settings_search.text.trim().is_empty() {
        return None;
    }
    let found = matches(labels, &s.settings_search.text);
    if found.is_empty() {
        return Some(
            label(&labels.no_results, 12., 400., theme.text_secondary)
                .w_full()
                .px(8.)
                .py(24.)
                .center(),
        );
    }
    let rows = found.into_iter().enumerate().map(|(i, (page, title))| {
        row()
            .key(format!("settings-result-{i}"))
            .role("button")
            .label(title)
            .focusable(true)
            .w_full()
            .min_h(40.)
            .px(10.)
            .py(4.)
            .radius(8.)
            .hover(theme.text.alpha(0.03))
            .child(
                col()
                    .min_w(0.)
                    .grow(1.)
                    .child(label(title, 14., 500., theme.text).truncate())
                    .child(
                        label(
                            page.title(Some(labels)),
                            11.,
                            400.,
                            theme.text_secondary.alpha(0.75),
                        )
                        .truncate(),
                    ),
            )
            .on_press_tx(move |s: &mut Desktop, tx| {
                s.clear_settings_search();
                page.select(s);
                tx.focus(page.key(), true);
            })
    });
    Some(
        col()
            .role("list")
            .label(&labels.search_label)
            .w_full()
            .gap(4.)
            .children(rows),
    )
}
