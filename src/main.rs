mod app;
mod config;
mod i18n;
mod ui;

use app::App;
use iced::{Size, window};

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .window(window::Settings {
            size: Size::new(800.0, 540.0),
            min_size: Some(Size::new(560.0, 420.0)),
            ..window::Settings::default()
        })
        .run()
}
