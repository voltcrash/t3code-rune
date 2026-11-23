use rune::{Color, El, Icon, Pixels, TextStyle, el, icon, row, text, tok};
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

#[derive(Clone, Copy, PartialEq)]
enum Placement {
    PreviousRun,
    CurrentRun,
    Composer,
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
    configured_feed: Option<u64>,
    viewport_revision: u64,
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
        image: image::RgbaImage,
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
        let mut rgba = image.as_raw().clone();
        let rgb =
            [color.r, color.g, color.b].map(|v| (rune::color::srgb_encode(v) * 255.).round() as u8);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[..3].copy_from_slice(&rgb);
            pixel[3] = (f32::from(pixel[3]) * color.a).round() as u8;
        }
        let mask = Pixels::new();
        let (w, h) = image.dimensions();
        mask.set(w, h, rgba);
        *cached = Some((color, mask.clone()));
        Some(mask)
    }
}

impl crate::Desktop {
    fn load_registry_icon(&mut self, watch: u64, url: &str) {
        if self.workspace.feed != Some(watch)
            || self.workspace.providers.configured_feed != Some(watch)
        {
            return;
        }
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
        image: Option<image::RgbaImage>,
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
    pub(super) fn viewport_changed(&mut self) -> bool {
        if self.configured_feed.is_some()
            && self
                .registry
                .values()
                .any(|icon| matches!(icon, RegistryIcon::Idle))
        {
            self.viewport_revision += 1;
            return true;
        }
        false
    }

    pub(super) fn set_watch(&mut self, watch: u64) {
        if self.configured_feed != Some(watch) {
            self.configured_feed = None;
            for state in self.registry.values_mut() {
                if !matches!(state, RegistryIcon::Ready { .. }) {
                    *state = RegistryIcon::Idle;
                }
            }
        }
    }

    pub(super) fn update(
        &mut self,
        watch: u64,
        instances: Vec<Instance>,
        art: HashMap<String, Art>,
    ) {
        self.configured_feed = Some(watch);
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

    pub(crate) fn driver_mark(
        &self,
        driver: &str,
        previous: bool,
        dark: bool,
        colors: Colors,
        size: f32,
    ) -> Option<El> {
        let tile = el().size(size, size).center();
        Some(if let Some(mark) = builtin(driver) {
            let glyph = icon(mark, size);
            if driver == "claudeAgent" {
                glyph.color(if previous {
                    rune::color::hex("#898989").unwrap()
                } else {
                    rune::color::hex("#d97757").unwrap()
                })
            } else {
                glyph.color(colors.rest).group_color(colors.hovered)
            }
        } else if driver == "antigravity" {
            tile.pixels_fit(antigravity(previous), rune::SurfaceFit::Fit)
        } else {
            let image = self.art.get(driver)?.pixels(dark, previous)?;
            tile.pixels_fit(image, rune::SurfaceFit::Fit)
        })
    }

    fn icon(
        &self,
        instance: &Instance,
        placement: Placement,
        dark: bool,
        colors: Colors,
        theme: &tok::Theme,
    ) -> El {
        let previous = placement == Placement::PreviousRun;
        let (size, opacity) = match placement {
            Placement::PreviousRun => (12., 0.35),
            Placement::CurrentRun => (14., 0.6),
            Placement::Composer => (16., 1.),
        };
        let tile = el().size(size, size).center();
        let compatible = instance.driver == "openaiCompatible";
        let remote = instance
            .registry
            .as_ref()
            .and_then(|url| self.registry.get(url));
        let logo = if let Some(mask) = remote.and_then(|image| image.mask(theme.text_secondary)) {
            tile.pixels_fit(&mask, rune::SurfaceFit::Fit)
        } else if let Some(mark) = self.driver_mark(&instance.driver, previous, dark, colors, size)
        {
            mark
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
        let logo = if let Some(watch) = self.configured_feed
            && let Some(url) = &instance.registry
            && matches!(remote, Some(RegistryIcon::Idle))
        {
            let url = url.clone();
            if placement == Placement::Composer {
                logo.key(format!("registry-{watch}-{url}")).on_visible(
                    move |s: &mut crate::Desktop, visible, tx| {
                        if visible {
                            s.load_registry_icon(watch, &url);
                        } else {
                            tx.live_only();
                        }
                    },
                )
            } else {
                logo.key(format!("registry-{watch}-{}-{url}", self.viewport_revision))
                    .on_drawn_layout_tx(move |s: &mut crate::Desktop, bounds, tx| {
                        if s.workspace.sidebar_intersects(bounds) {
                            s.load_registry_icon(watch, &url);
                        } else {
                            tx.live_only();
                        }
                    })
            }
        } else {
            logo
        };
        let mut mark = el().size(size, size).child(logo.opacity(opacity));
        if !previous && instance.badge {
            let badge_offset = if placement == Placement::Composer {
                -2.
            } else {
                -3.
            };
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
                el().pin(None, None, Some(badge_offset), Some(badge_offset))
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

    pub(super) fn composer_mark(&self, id: &str, dark: bool, theme: &tok::Theme) -> Option<El> {
        let ink = if dark { Color::WHITE } else { Color::BLACK };
        let colors = Colors {
            rest: ink,
            hovered: ink,
        };
        Some(
            self.icon(
                self.instances.get(id)?,
                Placement::Composer,
                dark,
                colors,
                theme,
            )
            .a11y_hidden(),
        )
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
                        .map(|instance| {
                            self.icon(instance, Placement::PreviousRun, dark, colors, theme)
                        }),
                )
                .child(self.icon(current, Placement::CurrentRun, dark, colors, theme)),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Desktop;
    use serde_json::json;

    #[test]
    fn registry_mask_preserves_source_alpha_across_theme_changes() {
        let icon = super::RegistryIcon::Ready {
            image: image::RgbaImage::from_raw(2, 1, vec![20, 30, 40, 0, 20, 30, 40, 128]).unwrap(),
            tinted: Default::default(),
        };
        let white = icon.mask(rune::Color::WHITE).unwrap();
        assert_eq!(white.rgba(), [255, 255, 255, 0, 255, 255, 255, 128]);
        let black = icon.mask(rune::Color::BLACK.alpha(0.5)).unwrap();
        assert_eq!(black.rgba(), [0, 0, 0, 0, 0, 0, 0, 64]);
    }

    #[test]
    fn registry_load_waits_for_the_feed_and_the_scroll_viewport() {
        use super::{Colors, RegistryIcon};
        use rune::{Cx, El, Env, Runtime, ScrollSpec, col, el, tok};

        fn view(s: &Desktop, _: &mut Cx<Desktop>) -> El {
            let ws = &s.workspace;
            let theme = tok::theme(false);
            let mark = ws.providers.icon(
                &ws.providers.instances["agent"],
                super::Placement::CurrentRun,
                false,
                Colors {
                    rest: theme.text,
                    hovered: theme.text,
                },
                &theme,
            );
            col().w_full().h_full().child(el().h(40.)).child(
                ws.sidebar_viewport(
                    col()
                        .key("list")
                        .w_full()
                        .h(80.)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(el().h(88.))
                        .child(mark)
                        .child(el().h(32.)),
                ),
            )
        }

        let mut app = Desktop::new();
        let instances = || {
            serde_json::from_value(json!([{"id":"agent", "driver":"acpRegistry",
            "initials":"A", "badge":false, "registry":"icon"}]))
            .unwrap()
        };
        app.workspace.feed = Some(1);
        app.workspace.set_feed("local".into(), 1);
        app.workspace
            .update_providers(1, instances(), Default::default());
        let mut rt = Runtime::new(app, view, (240., 240.), Env::default());
        let settle = |rt: &mut Runtime<Desktop>| {
            for _ in 0..120 {
                rt.frame(1. / 120.);
            }
        };
        settle(&mut rt);
        assert!(matches!(
            rt.state.workspace.providers.registry["icon"],
            RegistryIcon::Idle
        ));
        rt.update(|_, tx| tx.scroll_to_bottom("list"));
        settle(&mut rt);
        assert!(
            matches!(
                rt.state.workspace.providers.registry["icon"],
                RegistryIcon::Loading(_)
            ),
            "scroll into view"
        );
        rt.update(|s, _| {
            s.workspace.feed = Some(2);
            s.workspace.set_feed("local".into(), 2);
        });
        settle(&mut rt);
        assert!(
            matches!(
                rt.state.workspace.providers.registry["icon"],
                RegistryIcon::Idle
            ),
            "before first config"
        );
        rt.update(|s, _| {
            s.workspace
                .update_providers(2, instances(), Default::default())
        });
        settle(&mut rt);
        assert!(
            matches!(
                rt.state.workspace.providers.registry["icon"],
                RegistryIcon::Loading(_)
            ),
            "after first config"
        );
    }

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
