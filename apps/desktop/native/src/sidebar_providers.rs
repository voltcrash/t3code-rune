use rune::{Color, El, Icon, Image, Pixels, TextStyle, el, icon, row, text, tok};
use serde::Deserialize;
use std::{
    cell::{OnceCell, RefCell},
    collections::{HashMap, HashSet},
};

macro_rules! brand {
    ($name:literal, $view:expr, $path:literal, $attributes:literal) => {
        &Icon {
            name: $name,
            view: $view,
            body: concat!(
                "<path fill=\"currentColor\" ",
                $attributes,
                " d=\"",
                include_str!(concat!("../../../../assets/providers/", $path, ".svgpath")),
                "\"/>"
            ),
        }
    };
}

fn builtin(driver: &str) -> Option<&'static Icon> {
    match driver {
        "codex" | "openai" => Some(brand!(
            "openai",
            411.,
            "openai",
            "transform=\"translate(-100 -100)\" fill-rule=\"evenodd\""
        )),
        "claudeAgent" => Some(brand!("claude", 257., "claude", "")),
        "openrouter" => Some(brand!("openrouter", 24., "openrouter", "")),
        "anthropic" => Some(brand!("anthropic", 24., "anthropic", "")),
        "ollama" => Some(brand!("ollama", 24., "ollama", "")),
        _ => None,
    }
}

fn pixels(w: u32, h: u32, mut bytes: Vec<u8>, previous: bool) -> Pixels {
    if previous {
        for pixel in bytes.as_chunks_mut::<4>().0 {
            let gray = (0.2126 * f32::from(pixel[0])
                + 0.7152 * f32::from(pixel[1])
                + 0.0722 * f32::from(pixel[2]))
            .round() as u8;
            pixel[..3].fill(gray);
        }
    }
    let image = Pixels::new();
    image.set(w, h, bytes);
    image
}

fn antigravity(previous: bool) -> &'static Pixels {
    static IMAGES: [std::sync::OnceLock<Pixels>; 2] = [const { std::sync::OnceLock::new() }; 2];
    IMAGES[usize::from(previous)].get_or_init(|| {
        let rgba = image::load_from_memory(include_bytes!(
            "../../../../assets/providers/antigravity.png"
        ))
        .expect("bundled Antigravity logo")
        .into_rgba8();
        pixels(rgba.width(), rgba.height(), rgba.into_raw(), previous)
    })
}

#[derive(Clone, Copy)]
pub(super) struct Colors {
    pub(super) rest: Color,
    pub(super) hovered: Color,
}

#[derive(Deserialize)]
pub(super) struct Instance {
    id: String,
    driver: String,
    initials: String,
    badge: bool,
    registry: Option<String>,
    accent: Option<String>,
    ink: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Art {
    light: String,
    dark: String,
    #[serde(skip)]
    pixels: [[OnceCell<Option<Pixels>>; 2]; 2],
}

impl Art {
    fn pixels(&self, dark: bool, previous: bool) -> Option<&Pixels> {
        self.pixels[usize::from(dark)][usize::from(previous)]
            .get_or_init(|| {
                let source = if dark { &self.dark } else { &self.light };
                let (w, h, rgba) = rune::image::svg_rgba(source.as_bytes(), 42).ok()?;
                Some(pixels(w, h, rgba, previous))
            })
            .as_ref()
    }
}

#[derive(Default)]
pub(super) struct Catalog {
    environment: Option<String>,
    instances: HashMap<String, Instance>,
    art: HashMap<String, Art>,
    registry: HashMap<String, RegistryIcon>,
}

#[derive(Default)]
enum RegistryIcon {
    #[default]
    Idle,
    Loading(String),
    Missing,
    Ready {
        image: Image,
        tinted: RefCell<Option<(Color, Pixels)>>,
    },
}

impl RegistryIcon {
    fn mask(&self, color: Color) -> Option<Pixels> {
        let Self::Ready { image, tinted } = self else {
            return None;
        };
        let mut cached = tinted.borrow_mut();
        if let Some((prior, pixels)) = cached.as_ref()
            && *prior == color
        {
            return Some(pixels.clone());
        }
        let original = image.pixels();
        let mut rgba = original.rgba();
        let rgb =
            [color.r, color.g, color.b].map(|v| (rune::color::srgb_encode(v) * 255.).round() as u8);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[..3].copy_from_slice(&rgb);
            pixel[3] = (f32::from(pixel[3]) * color.a).round() as u8;
        }
        let mask = Pixels::new();
        let (w, h) = original.size();
        mask.set(w, h, rgba);
        *cached = Some((color, mask.clone()));
        Some(mask)
    }
}

impl crate::Desktop {
    fn load_registry_icon(&mut self, url: &str) {
        let Some(watch) = self.workspace.feed else {
            return;
        };
        let Some(state @ RegistryIcon::Idle) = self.workspace.providers.registry.get_mut(url)
        else {
            return;
        };
        let operation = uuid::Uuid::new_v4().to_string();
        *state = RegistryIcon::Loading(operation.clone());
        self.write_request(
            None,
            crate::Request::RegistryIcon {
                watch,
                url: url.to_owned(),
                operation,
            },
        );
    }

    pub(super) fn registry_icon_result(
        &mut self,
        watch: u64,
        url: &str,
        operation: &str,
        image: Option<Image>,
    ) {
        if self.workspace.feed != Some(watch) {
            return;
        }
        let Some(state) = self.workspace.providers.registry.get_mut(url) else {
            return;
        };
        if !matches!(state, RegistryIcon::Loading(id) if id == operation) {
            return;
        }
        *state = match image {
            Some(image) => RegistryIcon::Ready {
                image,
                tinted: RefCell::default(),
            },
            None => RegistryIcon::Missing,
        };
        self.workspace.refresh_provider_rows();
    }
}

impl Catalog {
    pub(super) fn set_environment(&mut self, id: String) {
        if self.environment.as_ref() != Some(&id) {
            *self = Self {
                environment: Some(id),
                ..Self::default()
            };
        }
    }

    pub(super) fn update(&mut self, instances: Vec<Instance>, art: HashMap<String, Art>) {
        let urls: HashSet<_> = instances
            .iter()
            .filter_map(|instance| instance.registry.as_ref())
            .collect();
        self.registry.retain(|url, _| urls.contains(url));
        for url in urls {
            self.registry.entry(url.clone()).or_default();
        }
        self.instances = instances
            .into_iter()
            .map(|instance| (instance.id.clone(), instance))
            .collect();
        self.art.extend(art);
    }

    fn icon(
        &self,
        instance: &Instance,
        previous: bool,
        dark: bool,
        colors: Colors,
        theme: &tok::Theme,
    ) -> El {
        let size = if previous { 12. } else { 14. };
        let tile = el().size(size, size).center();
        let compatible = instance.driver == "openaiCompatible";
        let remote = instance
            .registry
            .as_ref()
            .and_then(|url| self.registry.get(url));
        let logo = if let Some(mask) = remote.and_then(|image| image.mask(theme.text_secondary)) {
            tile.pixels_fit(&mask, rune::SurfaceFit::Fit)
        } else if let Some(mark) = builtin(&instance.driver) {
            let glyph = icon(mark, size);
            if instance.driver == "claudeAgent" {
                glyph.color(if previous {
                    rune::color::hex("#898989").unwrap()
                } else {
                    rune::color::hex("#d97757").unwrap()
                })
            } else {
                glyph.color(colors.rest).group_color(colors.hovered)
            }
        } else if instance.driver == "antigravity" {
            tile.pixels_fit(antigravity(previous), rune::SurfaceFit::Fit)
        } else if let Some(image) = self
            .art
            .get(&instance.driver)
            .and_then(|art| art.pixels(dark, previous))
        {
            tile.pixels_fit(image, rune::SurfaceFit::Fit)
        } else if compatible {
            tile.radius(4.)
                .fill(theme.card)
                .border(1., theme.card_border)
                .child(
                    text(instance.initials.chars().take(1).collect::<String>())
                        .font(TextStyle::new(8., 600.))
                        .color(theme.text),
                )
        } else {
            tile.child(
                text(&instance.initials)
                    .font(TextStyle::new(9., 600.))
                    .color(theme.text),
            )
        };
        let logo = if let Some(url) = &instance.registry
            && matches!(remote, Some(RegistryIcon::Idle))
        {
            let url = url.clone();
            logo.on_visible(move |s: &mut crate::Desktop, visible, _| {
                if visible {
                    s.load_registry_icon(&url);
                }
            })
        } else {
            logo
        };
        let mut mark = el()
            .size(size, size)
            .child(logo.opacity(if previous { 0.35 } else { 0.6 }));
        if !previous && instance.badge {
            let bg = instance
                .accent
                .as_deref()
                .and_then(rune::color::hex)
                .unwrap_or(theme.card);
            let fg = instance
                .ink
                .as_deref()
                .and_then(rune::color::hex)
                .unwrap_or(theme.text_secondary);
            mark = mark.child(
                el().pin(None, None, Some(-3.), Some(-3.))
                    .h(12.)
                    .min_w(12.)
                    .px(2.)
                    .radius(6.)
                    .center()
                    .fill(bg)
                    .border(1., theme.card)
                    .child(
                        text(&instance.initials)
                            .font(TextStyle::new(7., 600.))
                            .color(fg),
                    ),
            );
        }
        mark
    }

    pub(super) fn stack(
        &self,
        ids: &[String],
        dark: bool,
        colors: Colors,
        theme: &tok::Theme,
    ) -> Option<El> {
        let (current, previous) = ids.split_last()?;
        let current = self.instances.get(current)?;
        Some(
            row()
                .a11y_hidden()
                .center_y()
                .gap(6.)
                .children(
                    previous
                        .iter()
                        .filter_map(|id| self.instances.get(id))
                        .map(|instance| self.icon(instance, true, dark, colors, theme)),
                )
                .child(self.icon(current, false, dark, colors, theme)),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Desktop;
    use serde_json::json;

    #[test]
    fn rows_keep_their_provider_until_the_new_feeds_config_or_a_different_environment_arrives() {
        let mut app = Desktop::new();
        let workspace = |watch: u64, environment: &str| {
            serde_json::from_value(json!({
                "type":"workspace", "id":watch, "watch":watch, "environment_id":environment,
                "scratch_available":false, "projects":[], "threads":[]
            }))
            .unwrap()
        };
        let providers = |watch: u64, instances| {
            serde_json::from_value(json!({
                "type":"workspaceproviders","watch":watch,"instances":instances,"art":{}
            }))
            .unwrap()
        };
        let codex = json!([{"id":"work","driver":"codex","initials":"WC","badge":false}]);
        app.pending = true;
        app.receive(workspace(0, "local"));
        app.receive(providers(0, codex.clone()));
        app.workspace.refresh_request = Some(1);
        app.receive(workspace(1, "local"));
        assert!(app.workspace.providers.instances.contains_key("work"));
        app.receive(providers(1, json!([])));
        assert!(app.workspace.providers.instances.is_empty());
        app.receive(providers(1, codex));
        app.workspace.refresh_request = Some(2);
        app.receive(workspace(2, "remote"));
        assert!(app.workspace.providers.instances.is_empty());
    }
}
