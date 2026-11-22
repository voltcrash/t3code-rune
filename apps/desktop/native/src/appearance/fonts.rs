use super::{Desktop, Request, label};
use rune::{El, Family, TextStyle, col, tok};
use rune_kit::{Combobox, ComboboxOption, ComboboxState, Kit, Placement, Select, SelectAppearance};
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FontSlot {
    Sans,
    Code,
}

#[derive(Default)]
pub(crate) struct FontFamilies {
    pub(super) sans: String,
    pub(super) code: String,
    sans_picker: ComboboxState,
    code_picker: ComboboxState,
}

impl FontFamilies {
    pub(super) fn close(&mut self) {
        self.sans_picker.close();
        self.code_picker.close();
    }

    pub(crate) fn receive(&mut self, sans: String, code: String) {
        self.sans = sans;
        self.code = code;
    }
}

fn family_option(name: String) -> Option<ComboboxOption> {
    let css = if !name.contains(',') {
        name.clone()
    } else {
        // Rune's CSS parser supports either quote delimiter, with no escape syntax.
        let quote = ['"', '\'']
            .into_iter()
            .find(|quote| !name.contains(*quote))?;
        format!("{quote}{name}{quote}")
    };
    Some(ComboboxOption::new(css, name))
}

pub(super) fn saved_css(saved: &str) -> &str {
    if saved.contains(',') {
        // Earlier clients also accept user-entered CSS fallback lists.
        if let Some(font) = installed().iter().find(|font| font.label == saved) {
            return &font.id;
        }
    }
    saved
}

fn contains_code_glyphs(name: &str) -> bool {
    let Some(family) = fontique::Collection::default().family_by_name(name) else {
        return false;
    };
    let Some(face) = family.default_font() else {
        return false;
    };
    let Some(data) = face.load(None) else {
        return false;
    };
    let Some(map) = face.charmap_index().charmap(data.as_ref()) else {
        return false;
    };
    "iMW0@#. "
        .chars()
        .all(|c| map.map(c).is_some_and(|id| id != 0))
}

fn installed() -> &'static [ComboboxOption] {
    static FAMILIES: OnceLock<Vec<ComboboxOption>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut fonts = fontique::Collection::default();
        let mut names: Vec<_> = fonts
            .family_names()
            .filter(|name| !name.starts_with('.') && !name.starts_with('@'))
            .map(str::to_owned)
            .collect();
        names.sort_by_cached_key(|name| name.to_lowercase());
        let mut options = vec![ComboboxOption::new("", "System default")];
        options.extend(names.into_iter().filter_map(family_option));
        options
    })
}

pub(super) fn row(
    s: &Desktop,
    slot: FontSlot,
    narrow: bool,
    theme: &tok::Theme,
    dark: bool,
    popups: &mut Vec<El>,
) -> El {
    let fonts = &s.font_families;
    let (title, description, key, name, state) = match slot {
        FontSlot::Sans => (
            "Interface font",
            "Everything outside code blocks and the terminal.",
            "interface-font",
            &fonts.sans,
            &fonts.sans_picker,
        ),
        FontSlot::Code => (
            "Monospace font",
            "Code blocks, diffs, file previews, and the terminal.",
            "monospace-font",
            &fonts.code,
            &fonts.code_picker,
        ),
    };
    let default = match slot {
        FontSlot::Sans => "System default",
        FontSlot::Code => "System monospace",
    };
    let kit = Kit::neutral(dark);
    let field_label = format!("{title} family");
    let disabled = s.font_smoothing.is_none() || s.saving_client_appearance;
    let trigger = Select::new::<&str>(key, &[])
        .label(&field_label)
        .current(Some(if name.is_empty() { default } else { name }))
        .appearance(SelectAppearance::Field)
        .width(176.)
        .open(state.is_open())
        .disabled(disabled)
        .trigger(&kit);
    let options = if state.is_open() { installed() } else { &[] };
    let picker = Combobox::new(key, state, options, move |s: &mut Desktop| match slot {
        FontSlot::Sans => &mut s.font_families.sans_picker,
        FontSlot::Code => &mut s.font_families.code_picker,
    })
    .trigger(trigger)
    .label(field_label)
    .current(Some(saved_css(name)))
    .search_placeholder("Search fonts…")
    .placement(Placement::BottomEnd)
    .width(288.)
    .max_height(288.)
    .disabled(disabled)
    .on_choose(move |s, family, _| {
        let Some(font) = installed().iter().find(|option| option.id == family) else {
            return;
        };
        if matches!(slot, FontSlot::Code) && !family.is_empty() {
            let style = TextStyle::new(32., 400.).family(Family::Named(&font.id));
            if !contains_code_glyphs(&font.label) || !crate::terminal::fits_grid(style) {
                s.appearance_error = Some(format!(
                    "“{}” isn’t monospace. Choose a fixed-width font.",
                    font.label
                ));
                return;
            }
        }
        s.saving_client_appearance = s.write_request(
            None,
            Request::SetFontFamily {
                slot,
                family: if family.is_empty() { "" } else { &font.label },
            },
        );
    })
    .build(&kit, popups);
    let copy = col()
        .min_w(0.)
        .grow(1.)
        .gap(3.)
        .child(label(title, 14., 500., theme.text))
        .child(label(description, 12., 400., theme.text_secondary).wrap());
    let layout = if narrow {
        col()
    } else {
        rune::row().center_y()
    };
    layout
        .w_full()
        .gap(12.)
        .padding(if narrow { 12. } else { 16. })
        .child(copy)
        .child(picker)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_names_with_commas_remain_one_family() {
        assert_eq!(
            family_option("Acme, Mono".into()).unwrap().id,
            "\"Acme, Mono\""
        );
        assert_eq!(
            family_option("Acme \"One\", Mono".into()).unwrap().id,
            "'Acme \"One\", Mono'"
        );
        assert_eq!(
            saved_css("Missing Fixture, monospace"),
            "Missing Fixture, monospace"
        );
    }
}
