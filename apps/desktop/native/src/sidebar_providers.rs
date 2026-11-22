use rune::{Color, El, Icon, Pixels, TextStyle, el, icon, row, text, tok};
use serde::Deserialize;
use std::{cell::OnceCell, collections::HashMap};

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
    instances: HashMap<String, Instance>,
    art: HashMap<String, Art>,
}

impl Catalog {
    pub(super) fn update(&mut self, instances: Vec<Instance>, art: HashMap<String, Art>) {
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
        let logo = if let Some(mark) = builtin(&instance.driver) {
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
