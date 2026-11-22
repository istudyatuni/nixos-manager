pub mod cache_settings;
pub mod overview;
pub mod settings;

pub enum Screen {
    Overview,
    CacheSettings(Box<cache_settings::State>),
    Settings(Box<settings::State>),
}
