use std::fmt::Write;

use chrono::{DateTime, Local, Utc};
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use include_dir::{Dir, include_dir};

use super::locale::{LanguagePreference, Locale};

static LOCALES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/locales");

pub struct I18n {
    locale: Locale,
    active: Option<FluentBundle<FluentResource>>,
    fallback: FluentBundle<FluentResource>,
}

impl I18n {
    pub fn system() -> Self {
        let locale = Locale::system();
        Self {
            locale,
            active: (locale != Locale::English).then(|| load_bundle(locale)),
            fallback: load_bundle(Locale::English),
        }
    }

    pub fn set_language(&mut self, preference: LanguagePreference) -> bool {
        let locale = preference.resolve();
        if self.locale == locale {
            return false;
        }
        self.active = (locale != Locale::English).then(|| load_bundle(locale));
        self.locale = locale;
        true
    }

    pub fn text(&self, key: &str) -> String {
        self.format(key, None)
    }

    pub fn with_args(&self, key: &str, args: &FluentArgs<'_>) -> String {
        self.format(key, Some(args))
    }

    fn format(&self, key: &str, args: Option<&FluentArgs<'_>>) -> String {
        self.active
            .iter()
            .chain(std::iter::once(&self.fallback))
            .find_map(|bundle| {
                let value = bundle.get_message(key)?.value()?;
                let mut errors = Vec::new();
                let text = bundle.format_pattern(value, args, &mut errors);
                if !errors.is_empty() {
                    eprintln!("Fluent message {key}: {errors:?}");
                    return None;
                }
                Some(text.into_owned())
            })
            .unwrap_or_else(|| key.to_owned())
    }
}

fn load_bundle(locale: Locale) -> FluentBundle<FluentResource> {
    let tag = locale.tag();
    let dir = LOCALES.get_dir(tag).expect("embedded locale directory");
    let mut bundle = FluentBundle::new(vec![tag.parse().expect("valid locale identifier")]);
    bundle
        .add_function("DATETIME_LOCAL", date_time_local)
        .expect("register DATETIME_LOCAL");

    for file in dir
        .find("**/*.ftl")
        .expect("valid Fluent glob")
        .filter_map(|entry| entry.as_file())
    {
        let path = file.path().display();
        let source = file
            .contents_utf8()
            .unwrap_or_else(|| panic!("Invalid UTF-8 in {path}"));
        let resource = FluentResource::try_new(source.to_owned())
            .unwrap_or_else(|(_, errors)| panic!("Invalid Fluent resource {path}: {errors:?}"));
        bundle
            .add_resource(resource)
            .unwrap_or_else(|errors| panic!("Invalid Fluent messages in {path}: {errors:?}"));
    }
    bundle
}

fn date_time_local<'a>(positional: &[FluentValue<'a>], named: &FluentArgs<'_>) -> FluentValue<'a> {
    let ([FluentValue::Number(timestamp)], Some(FluentValue::String(pattern))) =
        (positional, named.get("pattern"))
    else {
        return FluentValue::Error;
    };
    if timestamp.value.fract() != 0.0 {
        return FluentValue::Error;
    }
    let Some(date) = DateTime::<Utc>::from_timestamp(timestamp.into(), 0) else {
        return FluentValue::Error;
    };

    let date = date.with_timezone(&Local);
    let mut formatted = String::new();
    match write!(formatted, "{}", date.format(pattern)) {
        Ok(()) => formatted.into(),
        Err(_) => FluentValue::Error,
    }
}
