use eframe::egui;
use material_terminal::app::MaterialTerminalApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("material-terminal")
            .with_title("Material Terminal")
            .with_inner_size([1160.0, 740.0])
            .with_min_inner_size([560.0, 360.0])
            .with_transparent(true),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Material Terminal",
        options,
        Box::new(|cc| Ok(Box::new(MaterialTerminalApp::new(cc)))),
    )
}
