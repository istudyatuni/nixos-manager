use serde_json::json;

use crate::attic::{
    FailureKind, Stage,
    tests::{NOW, fixed_time, jwt},
};

use super::*;

fn saved_cache() -> Metadata {
    Metadata {
        server_url: "https://cache.example.org/".to_owned(),
        server_name: "private".to_owned(),
        cache_name: "main".to_owned(),
        cache_expiration: fixed_time(),
    }
}

#[test]
fn opens_with_saved_values_and_an_empty_token_and_revalidates_the_selection() {
    let saved = saved_cache();
    let mut state = State::new(Some(&saved), Some(PathBuf::from("/test/attic")));
    assert_eq!(state.url, saved.server_url);
    assert_eq!(state.server_name, saved.server_name);
    assert_eq!(state.cache_name.as_deref(), Some("main"));
    assert!(state.token.is_empty());
    assert!(state.token_info.is_none());

    let token = jwt(
        NOW + 3600,
        json!({ "main": { "r": 1 }, "team-*": { "r": 1 } }),
    );
    state.update(Message::TokenChanged(token), fixed_time());
    assert_eq!(state.cache_name.as_deref(), Some("main"));
    assert!(
        state
            .token_info
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .has_patterns
    );

    let token = jwt(
        NOW + 3600,
        json!({ "next": { "r": 1 }, "main": { "w": 1 } }),
    );
    state.update(Message::TokenChanged(token), fixed_time());
    assert!(state.cache_name.is_none());
    assert!(matches!(
        state.update(Message::Connect, fixed_time()),
        Action::None
    ));
    assert!(matches!(
        state.status,
        Status::Invalid(ValidationError::CacheName)
    ));

    state.update(Message::CacheSelected("next".into()), fixed_time());
    state.update(
        Message::UrlChanged("https://another.example.org/".into()),
        fixed_time(),
    );
    state.update(Message::ServerNameChanged("another".into()), fixed_time());
    assert_eq!(state.cache_name.as_deref(), Some("next"));
    assert!(matches!(
        state.update(Message::Close, fixed_time()),
        Action::Close
    ));
    let reopened = State::new(Some(&saved), Some(PathBuf::from("/test/attic")));
    assert_eq!(reopened.url, saved.server_url);
    assert_eq!(reopened.server_name, saved.server_name);
    assert_eq!(reopened.cache_name.as_deref(), Some("main"));
    assert!(reopened.token.is_empty());
}

#[test]
fn defers_closing_during_connection_and_keeps_the_draft_on_failure() {
    let saved = saved_cache();
    let mut state = State::new(Some(&saved), Some(PathBuf::from("/test/attic")));
    let token = jwt(NOW + 3600, json!({ "main": { "r": 1 } }));
    state.update(Message::TokenChanged(token.clone()), fixed_time());

    assert!(matches!(
        state.update(Message::Connect, fixed_time()),
        Action::Connect(_, _)
    ));
    assert!(state.busy());
    assert!(matches!(
        state.update(Message::Connect, fixed_time()),
        Action::None
    ));
    assert!(matches!(
        state.update(Message::Close, fixed_time()),
        Action::None
    ));
    assert!(matches!(
        state.update(Message::TokenChanged(String::new()), fixed_time()),
        Action::None
    ));
    assert_eq!(state.token, token);
    let failure = Failure {
        stage: Stage::ApplyCache,
        kind: FailureKind::ConfigWrite,
    };
    assert!(!state.connected(&Err(failure), fixed_time()));
    assert!(!state.busy());
    assert_eq!(state.token, token);
    assert!(matches!(state.status, Status::ConnectFailed(error) if error.may_have_applied()));

    assert!(matches!(
        state.update(Message::Connect, fixed_time()),
        Action::Connect(_, _)
    ));
    assert!(matches!(
        state.update(Message::Close, fixed_time()),
        Action::None
    ));
    assert!(state.connected(&Ok(saved), fixed_time()));
    assert!(!state.busy());
    assert!(state.token.is_empty());
    assert!(state.token_info.is_none());
}
