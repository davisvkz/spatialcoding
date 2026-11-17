use spatialcoding::app::SpatialCodingApp;
use spatialcoding::cli::{self, Command, USAGE};

fn main() -> eframe::Result {
    env_logger::init();

    let startup = match cli::parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            return Ok(());
        }
        Ok(Command::Run(opts)) => opts.resolve(),
        Err(e) => Err(e),
    };
    let startup = startup.unwrap_or_else(|e| {
        eprintln!("error: {e}\n\n{USAGE}");
        std::process::exit(2);
    });

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
        Box::new(move |cc| {
            let mut app = SpatialCodingApp::new(cc);
            if let Some(path) = spatialcoding::settings_store::default_path() {
                app.enable_settings_persistence(path);
            }
            app.apply_startup(&startup);
            Ok(Box::new(app))
        }),
    )
}
