// Allow dead_code during incremental build: views, models, and accessors are
// defined ahead of full view composition (wired in Phase 4/5).
#![allow(dead_code)]

mod app;
mod components;
mod domain;
mod events;
mod fonts;
mod geometry;
mod guards;
mod icons;
mod layout;
mod paths;
mod services;
mod state;
mod style;
mod theme;
mod widgets;

use env_logger::Env;
use iced::{Size, window};
use image::ImageFormat;

use crate::app::{App, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::fonts::TERM_FONT_JET_BRAINS_BYTES;
use crate::icons::APP_ICON_DATA;

fn main() -> iced::Result {
    init_logging();

    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .antialiasing(true)
        .window(window::Settings {
            decorations: false,
            min_size: Some(Size {
                width: MIN_WINDOW_WIDTH,
                height: MIN_WINDOW_HEIGHT,
            }),
            icon: window::icon::from_file_data(
                APP_ICON_DATA,
                Some(ImageFormat::Png),
            )
            .ok(),
            ..window::Settings::default()
        })
        .resizable(true)
        .font(TERM_FONT_JET_BRAINS_BYTES)
        .subscription(App::subscription)
        .run()
}

/// Initialize logging; when OTTY_LOG_FILE is set, logs (and thread panics)
/// go to that file at debug level — the only observable channel on Windows,
/// where the GUI has no console attached.
fn init_logging() {
    let mut builder =
        env_logger::Builder::from_env(Env::default().default_filter_or("info"));

    if let Ok(path) = std::env::var("OTTY_LOG_FILE") {
        match std::fs::File::create(&path) {
            Ok(file) => {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
                builder.filter_level(log::LevelFilter::Debug);
                // Verbose tracing for otty's own modules only; keeps wgpu
                // and other dependencies at debug level.
                builder.filter(Some("otty"), log::LevelFilter::Trace);
            },
            Err(err) => {
                eprintln!("failed to create OTTY_LOG_FILE {path}: {err}");
            },
        }
    }

    builder.format_timestamp_millis().init();

    log::info!("otty starting (diag build 3)");

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("thread panicked: {info}");
        default_hook(info);
    }));
}
