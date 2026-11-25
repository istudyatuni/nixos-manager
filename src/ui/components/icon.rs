use iced::{
    Font,
    widget::{pick_list, text},
};

mod lucide {
    include!(concat!(env!("OUT_DIR"), "/src/ui/components/icon.rs"));
}

pub use lucide::{
    FONT, chevron_down, chevron_up, clear_selection, close, cloud_download, copy, help, menu, play,
    refresh, settings, stop, trash,
};

pub const TYPEFACE: Font = Font::with_name("lucide");

pub fn glyph(name: &str) -> &'static str {
    lucide::ALL_ICONS
        .iter()
        .find_map(|&(key, glyph)| (key == name).then_some(glyph))
        .unwrap_or_else(|| panic!("Icon {name:?} is missing from static/icons.toml"))
}

pub fn code_point(name: &str) -> char {
    glyph(name).chars().next().expect("Lucide code point")
}

pub fn pick_list_handle() -> pick_list::Handle<Font> {
    pick_list::Handle::Static(pick_list::Icon {
        font: TYPEFACE,
        code_point: code_point("chevron_down"),
        size: Some(14.0.into()),
        line_height: text::LineHeight::default(),
        shaping: text::Shaping::Basic,
    })
}

#[cfg(test)]
pub use lucide::{ALL_ICONS, render};
