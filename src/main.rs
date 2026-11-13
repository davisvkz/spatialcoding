use spatialcoding::app::SpatialCodingApp;

fn main() -> eframe::Result {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("SpatialCoding"),
        ..Default::default()
    };

    eframe::run_native(
        "SpatialCoding",
        options,
        Box::new(|cc| Ok(Box::new(SpatialCodingApp::new(cc)))),
    )
}
