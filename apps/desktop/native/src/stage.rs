use rune::{El, Pixels, el};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Stage {
    Dev,
    Nightly,
    #[default]
    Beta,
}

pub(super) fn sidebar_grain(width: f32, height: f32) -> El {
    const SIZE: f32 = 256.;
    static TILE: OnceLock<Pixels> = OnceLock::new();
    let pixels = TILE.get_or_init(|| {
        let (w, h, rgba) = rune::image::svg_rgba(
            include_bytes!("../../../../assets/surface-grain.svg"),
            SIZE as u32,
        )
        .expect("valid sidebar grain");
        let pixels = Pixels::new();
        pixels.set(w, h, rgba);
        pixels
    });
    let mut layer = el().absolute(0., 0.).size(width, height);
    for y in 0..(height / SIZE).ceil() as usize {
        for x in 0..(width / SIZE).ceil() as usize {
            layer = layer.child(
                el().absolute(x as f32 * SIZE, y as f32 * SIZE)
                    .size(SIZE, SIZE)
                    .pixels(pixels),
            );
        }
    }
    layer
}

impl Stage {
    pub fn has_art(self) -> bool {
        !matches!(self, Self::Beta)
    }

    pub fn backdrop(self, dark: bool, width: f32) -> Option<El> {
        // Tiles follow the 8192 × 96 web artwork at its fixed 80-point height.
        const TILES: usize = 27;
        static DEV_LIGHT: [OnceLock<Pixels>; TILES] = [const { OnceLock::new() }; TILES];
        static DEV_DARK: [OnceLock<Pixels>; TILES] = [const { OnceLock::new() }; TILES];
        static NIGHTLY: [OnceLock<Pixels>; TILES] = [const { OnceLock::new() }; TILES];
        let (cache, art) = match self {
            Self::Dev => (
                if dark { &DEV_DARK } else { &DEV_LIGHT },
                include_str!("../../../../assets/dev/sidebar.svg"),
            ),
            Self::Nightly => (
                &NIGHTLY,
                include_str!("../../../../assets/nightly/sidebar.svg"),
            ),
            Self::Beta => return None,
        };
        let mut strip = el().absolute(0., 0.).size(width, 80.);
        for (index, tile) in cache
            .iter()
            .take((width / 256.).ceil() as usize)
            .enumerate()
        {
            let pixels = tile.get_or_init(|| rasterize(art, dark, index));
            strip = strip.child(
                el().absolute(index as f32 * 256., 0.)
                    .size(256., 80.)
                    .pixels(pixels),
            );
        }
        Some(strip)
    }
}

fn rasterize(art: &str, dark: bool, index: usize) -> Pixels {
    let mut svg = art.replacen(
        "0 0 8192 96",
        &format!("{} 0 307.2 96", index as f64 * 307.2),
        1,
    );
    for (token, color) in [
        (
            "var(--stage-art-bottom)",
            if dark { "#101f6e" } else { "#1538d0" },
        ),
        ("var(--stage-art-celeste-highlight)", "#d2ffff"),
        ("var(--stage-art-celeste-secondary)", "#48dcf5"),
        ("var(--stage-art-grid-line)", "#eaf6ff"),
        ("var(--stage-art-highlight)", "#d4f6ff"),
        ("var(--stage-art-line)", "#ddf7ff"),
        (
            "var(--stage-art-mid)",
            if dark { "#2050ae" } else { "#347ff8" },
        ),
        ("var(--stage-art-secondary)", "#65c8ff"),
        ("var(--stage-art-tertiary)", "#7c8bff"),
        (
            "var(--stage-art-top)",
            if dark { "#3a7ad1" } else { "#67c2ff" },
        ),
        ("var(--stage-art-violet-highlight)", "#d9d8ff"),
        ("var(--stage-night-bottom)", "#07152f"),
        ("var(--stage-night-glow-highlight)", "#5165d8"),
        ("var(--stage-night-glow-secondary)", "#283075"),
        ("var(--stage-night-highlight)", "#4ea4ff"),
        ("var(--stage-night-line)", "#e4eaff"),
        ("var(--stage-night-mid)", "#151443"),
        ("var(--stage-night-secondary)", "#696fea"),
        ("var(--stage-night-sparkle)", "#c8d7ff"),
        ("var(--stage-night-tertiary)", "#a85bea"),
        ("var(--stage-night-top)", "#32155b"),
    ] {
        svg = svg.replace(token, color);
    }
    let (w, h, mut rgba) =
        rune::image::svg_rgba(svg.as_bytes(), 768).expect("valid bundled sidebar artwork");
    // Composite the web backdrop’s two fades into the alpha channel over the native sidebar.
    let stops = [
        (0.28, 1.),
        (0.40, 0.90),
        (0.52, 0.70),
        (0.64, 0.42),
        (0.75, 0.18),
        (0.85, 0.04),
        (0.93, 0.),
    ];
    for (y, row) in rgba.chunks_exact_mut(4 * w as usize).enumerate() {
        let t = (y as f32 + 0.5) / h as f32;
        let fade = stops
            .windows(2)
            .find_map(|p| {
                let [(a, from), (b, to)] = [p[0], p[1]];
                (t <= b).then(|| from + (to - from) * ((t - a) / (b - a)).clamp(0., 1.))
            })
            .unwrap_or(0.);
        let mask = ((0.92 - t) / (0.92 - 0.55)).clamp(0., 1.);
        for pixel in row.as_chunks_mut::<4>().0 {
            pixel[3] = (pixel[3] as f32 * fade * mask).round() as u8;
        }
    }
    let pixels = Pixels::new();
    pixels.set(w, h, rgba);
    pixels
}
