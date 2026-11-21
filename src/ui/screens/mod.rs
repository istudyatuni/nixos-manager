pub mod overview;
pub mod settings;

pub enum Screen {
    Overview,
    Settings(settings::State),
}
