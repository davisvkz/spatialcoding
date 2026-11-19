use spatialcoding::app::SpatialCodingApp;
use spatialcoding::cli::{self, Command, USAGE};

fn main() -> eframe::Result {
    env_logger::init();

    let startup = match cli::parse(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{USAGE}");
            return Ok(());
        }
        Ok(Command::Mcp) => return run_mcp(),
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
            app.enable_bridge();
            app.apply_startup(&startup);
            Ok(Box::new(app))
        }),
    )
}

/// `--mcp`: speak MCP on stdio and forward every tool call to the running app.
#[cfg(unix)]
fn run_mcp() -> eframe::Result {
    use spatialcoding::server::{bridge, mcp};
    let mut backend = mcp::AppBackend::new(bridge::socket_dir());
    let stdin = std::io::stdin().lock();
    if let Err(e) = mcp::serve(stdin, std::io::stdout().lock(), &mut backend) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(not(unix))]
fn run_mcp() -> eframe::Result {
    eprintln!("error: --mcp needs a Unix socket and is not available on this platform");
    std::process::exit(2);
}
