use eframe::egui;
use material_terminal::app::MaterialTerminalApp;

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("material-terminal")
        .with_title("Material Terminal")
        .with_inner_size([1160.0, 760.0])
        .with_min_inner_size([620.0, 420.0])
        .with_transparent(false);

    #[cfg(target_os = "macos")]
    {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false)
            .with_titlebar_buttons_shown(true);
    }

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Material Terminal",
        options,
        Box::new(|cc| Ok(Box::new(MaterialTerminalApp::new(cc)))),
    )
}
