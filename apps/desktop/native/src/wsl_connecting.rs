use rune::{
    App, AppSpec, Chrome, Color, Cx, El, Family, Icon, LoopSpec, TextStyle, WindowOptions, col, el,
    icon, text,
};

const SIZE: (f32, f32) = (360.0, 220.0);
const LABEL: &str = "Connecting to WSL…";
const ARC: Icon = Icon {
    name: "connecting-arc",
    view: 32.0,
    body: r#"<path d="M5.747 5.747 A14.5 14.5 0 0 1 26.253 5.747" fill="none" stroke="currentColor" stroke-width="3"/>"#,
};

fn view(_: &(), cx: &mut Cx<()>) -> El {
    let (background, accent, label) = if cx.system_dark() {
        (Color::BLACK, "#f8fafc", "#9ca3af")
    } else {
        (Color::WHITE, "#1f2937", "#6b7280")
    };
    let accent = rune::color::hex(accent).unwrap();
    let spinner = el()
        .role("progressbar")
        .label(LABEL)
        .size(32.0, 32.0)
        .radius(16.0)
        .border(3.0, accent.alpha(0.18))
        .child(
            icon(&ARC, 32.0)
                .absolute(0.0, 0.0)
                .color(accent)
                .looping(LoopSpec::rotation(0.8)),
        );

    col()
        .w_full()
        .h_full()
        .window_drag()
        .center()
        .gap(18.0)
        .fill(background)
        .child(spinner)
        .child(
            text(LABEL)
                .font(
                    TextStyle::new(13.0, 400.0)
                        .family(Family::Named("Segoe UI, .AppleSystemUIFont")),
                )
                .color(rune::color::hex(label).unwrap()),
        )
}

pub fn window() -> App<()> {
    let spec = AppSpec::new("T3 Code", SIZE, || (), view);
    App::new(spec)
        .chrome(Chrome {
            drag_regions: true,
            ..Chrome::DEFAULT
        })
        .window_options(WindowOptions {
            borderless: true,
            resizable: false,
            min_size: Some(SIZE),
            max_size: Some(SIZE),
            ..Default::default()
        })
}
