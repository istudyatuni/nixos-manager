pub mod overview;
pub mod settings;

pub enum Screen {
    Overview,
    Settings(Box<settings::State>),
}
