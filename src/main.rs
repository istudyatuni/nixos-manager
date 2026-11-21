mod app;
mod config;
mod git;
mod i18n;
mod ui;

use app::App;
use iced::{Size, window};

fn main() -> iced::Result {
    let mut args = std::env::args_os().skip(1);
    if args
        .next()
        .is_some_and(|arg| arg == git::CREDENTIAL_HELPER_FLAG)
    {
        if args.next().is_some_and(|action| action == "get") && git::write_credential().is_err() {
            std::process::exit(1);
        }
        return Ok(());
    }

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window(window::Settings {
            size: Size::new(800.0, 540.0),
            min_size: Some(Size::new(560.0, 420.0)),
            ..window::Settings::default()
        })
        .run()
}
