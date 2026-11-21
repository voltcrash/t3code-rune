use rune::{Color, El, Pixels, TextStyle, el, oklch, text};
use serde::Deserialize;
use std::cell::OnceCell;

#[derive(Deserialize)]
pub(super) struct Palette {
    light: [f32; 3],
    dark: [f32; 3],
}

impl Palette {
    fn foreground(&self, dark: bool) -> Color {
        let [lightness, chroma, hue] = if dark { self.dark } else { self.light };
        oklch(lightness / 100., chroma, hue)
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(super) enum ProjectIcon {
    Monogram {
        text: String,
        color: Palette,
    },
    Emoji {
        emoji: String,
    },
    Lucide {
        body: String,
        color: Palette,
        #[serde(skip)]
        pixels: [OnceCell<Option<Pixels>>; 2],
    },
}

impl ProjectIcon {
    pub(super) fn view(&self, dark: bool) -> El {
        let tile = el().size(16., 16.).center().a11y_hidden();
        match self {
            Self::Monogram {
                text: initials,
                color,
            } => {
                let ink = color.foreground(dark);
                tile.clip_rounded(4.).fill(ink.alpha(0.14)).child(
                    text(initials)
                        .font(TextStyle::new(8.25, 700.).mono().tracking(0.8))
                        .color(ink),
                )
            }
            Self::Emoji { emoji } => tile.child(text(emoji).font(TextStyle::new(12.8, 400.))),
            Self::Lucide {
                body,
                color,
                pixels,
            } => {
                let image = pixels[usize::from(dark)].get_or_init(|| {
                    let ink = color.foreground(dark).hex();
                    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="{ink}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#);
                    let (width, height, bytes) = rune::image::svg_rgba(svg.as_bytes(), 48).ok()?;
                    let image = Pixels::new();
                    image.set(width, height, bytes);
                    Some(image)
                });
                match image {
                    Some(image) => tile.pixels(image),
                    None => tile,
                }
            }
        }
    }
}
