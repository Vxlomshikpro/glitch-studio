use clap::Parser;
use glitch_studio::cli::{self, Cli};

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,glitch_studio=info".into()),
        )
        .init();

    let args = Cli::parse();

    // Только --output означает пакетный режим; один --input можно и через GUI.
    let cli_mode = args.output.is_some()
        || args.preset.is_some()
        || args.effect.is_some()
        || args.list_effects
        || args.list_presets;

    if cli_mode {
        return match cli::run(&args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ошибка: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    match glitch_studio::ui::run_gui(args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("не удалось запустить окно: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
