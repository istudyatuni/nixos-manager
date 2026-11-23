pub mod build_history;
pub mod cache_settings;
pub mod overview;
pub mod settings;

pub enum Screen {
    Overview,
    BuildHistory(build_history::State),
    CacheSettings(Box<cache_settings::State>),
    Settings(Box<settings::State>),
}
