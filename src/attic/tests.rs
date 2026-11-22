use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};
use tokio::sync::mpsc;

use super::*;

pub(crate) const NOW: i64 = 1_795_367_982;
const TOKEN_CACHES_CLAIM: &str = "https://jwt.attic.rs/v1";

pub(crate) fn fixed_time() -> DateTime<Utc> {
    DateTime::from_timestamp(NOW, 0).unwrap()
}

fn encoded(payload: &str) -> String {
    format!(
        "{}.{}.c2lnbmF0dXJl",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(payload)
    )
}

pub(crate) fn jwt(expiration: i64, caches: Value) -> String {
    encoded(
        &json!({
            "exp": expiration,
            "nbf": NOW,
            TOKEN_CACHES_CLAIM: { "caches": caches },
        })
        .to_string(),
    )
}

#[test]
fn validates_addresses_and_attic_names() {
    assert_eq!(
        server_url(" https://cache.example.org:123/foo ")
            .unwrap()
            .as_str(),
        "https://cache.example.org:123/foo"
    );
    assert!(server_url("HTTP://[::1]:8080").is_some());
    for url in [
        "",
        "attic",
        "https:attic",
        "https:///attic",
        "https://",
        "ssh://cache.example.org",
        "https://u:p@cache.example.org",
        "https://@cache.example.org",
        "https://cache.example.org?",
        "https://cache.example.org#fragment",
        "https://cache.example.org#",
        "https://cache.example.org/a b",
        "https://cache.example.org\\path",
    ] {
        assert!(server_url(url).is_none(), "{url}");
    }
    assert!(valid_name("mIX_Cache-1+2"));
    assert!(valid_name(&"a".repeat(50)));
    for name in [
        "",
        "_main",
        "a.b",
        "a/b",
        "a:b",
        "team-*",
        "кеш",
        &"a".repeat(51),
    ] {
        assert!(!valid_name(name), "{name}");
    }
}

#[test]
fn selects_the_first_path_entry_without_testing_executability() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let path = env::join_paths([first.path(), second.path()]).unwrap();
    assert_eq!(find(None), None);
    assert_eq!(find(Some(&path)), None);
    fs::write(second.path().join("attic"), "").unwrap();
    assert_eq!(find(Some(&path)), Some(second.path().join("attic")));
    fs::create_dir(first.path().join("attic")).unwrap();
    assert_eq!(find(Some(&path)), Some(first.path().join("attic")));
}

#[test]
fn lists_sorted_literal_cache_names_with_read_access() {
    let token = jwt(
        NOW + 3600,
        json!({
            "z-last": { "r": 1 },
            "main": { "w": 1, "r": 1 },
            "write-only": { "w": 1 },
            "false": { "r": 0 },
            "not-one": { "r": 2 },
            "-invalid": { "r": 1 },
            "team-*": { "r": 1 },
        }),
    );
    let token = Token::parse(&token, fixed_time()).unwrap();
    assert_eq!(token.cache_names, ["main", "z-last"]);
    assert!(token.has_patterns);

    let token = jwt(NOW + 3600, json!({ "*": { "w": 1 } }));
    let token = Token::parse(&token, fixed_time()).unwrap();
    assert!(token.cache_names.is_empty());
    assert!(token.has_patterns);
}

#[test]
fn reports_token_errors_separately() {
    for (token, error) in [
        ("e30.%!.c2ln".to_owned(), TokenError::Malformed),
        (encoded("not JSON"), TokenError::Malformed),
        (encoded("{}"), TokenError::Expiration),
        (
            encoded(r#"{"exp":9223372036854775807}"#),
            TokenError::Expiration,
        ),
        (encoded(r#"{"exp":253402300800}"#), TokenError::Expiration),
        (jwt(NOW, json!({})), TokenError::Expired),
        (jwt(NOW - 1, json!({})), TokenError::Expired),
        (
            encoded(&json!({ "nbf": NOW + 1, "exp": NOW + 3600 }).to_string()),
            TokenError::NotYetValid,
        ),
        (
            encoded(&json!({ "exp": NOW + 3600 }).to_string()),
            TokenError::Claims,
        ),
        (
            encoded(&json!({ "exp": NOW + 3600, TOKEN_CACHES_CLAIM: {"caches": []} }).to_string()),
            TokenError::Claims,
        ),
    ] {
        assert_eq!(Token::parse(&token, fixed_time()).err(), Some(error));
    }

    let token = jwt(NOW + 3600, json!({ "main": { "w": 1 }, "new": { "r": 1 } }));
    let request = Connection::new(
        "https://cache.example.org",
        "private",
        &token,
        Some("main"),
        fixed_time(),
    );
    assert_eq!(request.err(), Some(ValidationError::CacheName));
}

#[test]
fn checks_expiration_boundaries_with_fixed_time() {
    for (offset, expected) in [
        (-1, Some(ExpirationWarning::Expired)),
        (0, Some(ExpirationWarning::Expired)),
        (1, Some(ExpirationWarning::Expiring)),
        (86_399, Some(ExpirationWarning::Expiring)),
        (86_400, None),
        (86_401, None),
    ] {
        let metadata = Metadata {
            server_name: "private".into(),
            server_url: "https://cache.example.org/".into(),
            cache_name: "main".into(),
            cache_expiration: DateTime::from_timestamp(NOW + offset, 0).unwrap(),
        };
        assert_eq!(metadata.warning(fixed_time()), expected);
    }
}

#[test]
fn reads_and_forgets_cache_metadata_independently_of_repository_settings() {
    let home = tempdir().unwrap();
    let directory = home.path().join(CONFIG_DIRECTORY);
    assert_eq!(load(Some(home.path())).unwrap(), None);
    assert!(forget(Some(home.path())).is_ok());

    fs::create_dir_all(&directory).unwrap();
    let settings = directory.join("config.json");
    let attic_file = directory.join(METADATA_FILE);
    fs::write(&settings, "unrelated repository settings").unwrap();
    fs::write(&attic_file, "invalid metadata").unwrap();
    assert!(load(Some(home.path())).is_err());
    forget(Some(home.path())).unwrap();
    assert_eq!(load(Some(home.path())).unwrap(), None);
    assert_eq!(
        fs::read_to_string(settings).unwrap(),
        "unrelated repository settings"
    );
}

struct Fixture {
    home: TempDir,
    attic: PathBuf,
    user_config: PathBuf,
    metadata_directory: PathBuf,
    log: PathBuf,
    xdg_log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempdir().unwrap();
        let attic = home.path().join("attic");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attic.sh"),
            &attic,
        )
        .unwrap();
        fs::set_permissions(&attic, fs::Permissions::from_mode(0o700)).unwrap();
        let user_config = home.path().join("user-config");
        let metadata_directory = home.path().join(CONFIG_DIRECTORY);
        let log = home.path().join("calls");
        let xdg_log = home.path().join("xdg");
        Self {
            home,
            attic,
            user_config,
            metadata_directory,
            log,
            xdg_log,
        }
    }

    async fn run(
        &self,
        scenario: &str,
        token: &str,
        cache: &str,
        command_timeout: Duration,
    ) -> (
        Result<Metadata, Failure>,
        Vec<crate::command_runner::LogLine>,
    ) {
        let connection = Connection::new(
            "https://cache.example.org/",
            "private",
            token,
            Some(cache),
            fixed_time(),
        )
        .unwrap();
        let (sender, mut receiver) = mpsc::channel(32);
        let runner = CommandRunner::new(sender);
        let result = connect_with(
            || {
                let mut command = Command::new(&self.attic);
                command
                    .env("XDG_CONFIG_HOME", &self.user_config)
                    .env("ATTIC_TEST_USER_CONFIG", &self.user_config)
                    .env("ATTIC_TEST_METADATA_DIR", &self.metadata_directory)
                    .env("ATTIC_TEST_LOG", &self.log)
                    .env("ATTIC_TEST_XDG_LOG", &self.xdg_log)
                    .env("ATTIC_TEST_SCENARIO", scenario);
                command
            },
            Some(self.home.path()),
            runner,
            connection,
            command_timeout,
        )
        .await;
        let mut journal = Vec::new();
        while let Ok(line) = receiver.try_recv() {
            journal.push(line);
        }
        assert!(
            journal
                .iter()
                .all(|line| !line.text.contains(token) && !line.text.contains("existing-token"))
        );
        assert!(!format!("{result:?}").contains(token));
        (result, journal)
    }

    fn calls(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn xdg_paths(&self) -> Vec<PathBuf> {
        fs::read_to_string(&self.xdg_log)
            .unwrap()
            .lines()
            .map(PathBuf::from)
            .collect()
    }

    fn files(&self) -> [PathBuf; 3] {
        ["attic/config.toml", "nix/netrc", "nix/nix.conf"].map(|name| self.user_config.join(name))
    }

    fn seed(&self) -> Metadata {
        for (file, contents) in
            self.files()
                .iter()
                .zip(["existing-token", "old netrc", "old nix.conf"])
        {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, contents).unwrap();
        }
        let old = Metadata {
            cache_name: "previous".into(),
            cache_expiration: fixed_time(),
            server_name: "private".into(),
            server_url: "https://cache.example.org/".into(),
        };
        config::prepare_json(&self.metadata_directory, &old)
            .unwrap()
            .persist(self.metadata_directory.join(METADATA_FILE))
            .unwrap();
        old
    }

    fn stored(&self) -> Option<Metadata> {
        load(Some(self.home.path())).unwrap()
    }
}

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn connects_renews_switches_and_forgets_a_cache() {
    let fixture = Fixture::new();
    let token = jwt(NOW + 3600, json!({ "main": { "r": 1 } }));
    let (result, journal) = fixture.run("success", &token, "main", TEST_TIMEOUT).await;
    let metadata = result.unwrap();
    assert_eq!(fixture.stored(), Some(metadata.clone()));
    assert_eq!(metadata.cache_name, "main");

    let calls = fixture.calls();
    assert_eq!(
        calls.lines().collect::<Vec<_>>(),
        [
            "verify login --set-default private https://cache.example.org/ ***",
            "verify cache info private:main",
            "apply login --set-default private https://cache.example.org/ ***",
            "apply use private:main",
        ],
    );
    assert_eq!(journal.len(), 4);
    assert!(!calls.contains(&token));
    let paths = fixture.xdg_paths();
    assert_eq!(paths[0], paths[1]);
    assert!(!paths[0].exists());
    assert_eq!(paths[2..], [fixture.user_config.as_path(); 2]);
    let metadata_file = fixture.metadata_directory.join(METADATA_FILE);
    let json = fs::read_to_string(&metadata_file).unwrap();
    assert_eq!(
        fs::metadata(&metadata_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        serde_json::from_str::<Value>(&json).unwrap(),
        json!({
            "cache_name": "main",
            "cache_expiration": metadata.expiration_rfc3339(),
            "server_name": "private",
            "server_url": "https://cache.example.org/",
        })
    );
    assert!(!json.contains(&token));
    let [profile, netrc, nix_conf] = fixture.files();
    assert!(fs::read_to_string(&profile).unwrap().contains(&token));
    assert!(fs::read_to_string(&netrc).unwrap().contains(&token));
    assert!(fs::read_to_string(&nix_conf).unwrap().ends_with("/main\n"));

    let renewed = jwt(
        NOW + 86_401,
        json!({ "main": { "r": 1 }, "next": { "r": 1 } }),
    );
    let (result, _) = fixture.run("success", &renewed, "main", TEST_TIMEOUT).await;
    assert_eq!(result.unwrap().warning(fixed_time()), None);
    assert!(fs::read_to_string(&netrc).unwrap().contains(&renewed));

    let (result, _) = fixture.run("success", &renewed, "next", TEST_TIMEOUT).await;
    assert_eq!(result.unwrap().cache_name, "next");
    assert!(fs::read_to_string(&nix_conf).unwrap().ends_with("/next\n"));
    let previous = fixture.files().map(|file| fs::read(file).unwrap());
    forget(Some(fixture.home.path())).unwrap();
    assert_eq!(fixture.stored(), None);
    assert_eq!(
        fixture.files().map(|file| fs::read(file).unwrap()),
        previous
    );
}

#[tokio::test]
async fn leaves_user_files_intact_on_failed_verification() {
    for (scenario, stage, kind, calls) in [
        (
            "unauthorized",
            Stage::VerifyCache,
            FailureKind::AccessDenied,
            2,
        ),
        ("network", Stage::VerifyCache, FailureKind::Cli, 2),
        ("timeout", Stage::VerifyCache, FailureKind::Timeout, 2),
        (
            "verify-login-write-failed",
            Stage::VerifyLogin,
            FailureKind::ConfigWrite,
            1,
        ),
    ] {
        let fixture = Fixture::new();
        let metadata = fixture.seed();
        let before = fixture.files().map(|file| fs::read(file).unwrap());
        let token = jwt(NOW + 3600, json!({ "main": { "r": 1 } }));
        let duration = if scenario == "timeout" {
            Duration::from_secs(1)
        } else {
            TEST_TIMEOUT
        };
        let (result, journal) = fixture.run(scenario, &token, "main", duration).await;
        let error = result.unwrap_err();
        assert_eq!(error, Failure { stage, kind });
        assert!(!error.may_have_applied());
        assert_eq!(fixture.calls().lines().count(), calls);
        assert_eq!(journal.len(), calls);
        assert!(fixture.xdg_paths().iter().all(|path| !path.exists()));
        assert_eq!(fixture.files().map(|file| fs::read(file).unwrap()), before);
        assert_eq!(fixture.stored(), Some(metadata));
        assert_eq!(
            fs::read_dir(&fixture.metadata_directory).unwrap().count(),
            1
        );
    }
}

#[tokio::test]
async fn reports_partial_application_and_preserves_previous_metadata_until_retry() {
    for (scenario, stage, kind) in [
        (
            "apply-login-write-failed",
            Stage::ApplyLogin,
            FailureKind::ConfigWrite,
        ),
        ("partial-use", Stage::ApplyCache, FailureKind::ConfigWrite),
        (
            "metadata-commit-failed",
            Stage::Save,
            FailureKind::MetadataWrite,
        ),
    ] {
        let fixture = Fixture::new();
        let metadata = fixture.seed();
        let [profile, netrc, nix_conf] = fixture.files();
        let token = jwt(NOW + 3600, json!({ "main": { "r": 1 } }));
        let (result, _) = fixture.run(scenario, &token, "main", TEST_TIMEOUT).await;
        let error = result.unwrap_err();
        assert_eq!(error, Failure { stage, kind });
        assert!(error.may_have_applied());
        assert_eq!(fixture.stored(), Some(metadata));
        assert_eq!(
            fs::read_dir(&fixture.metadata_directory).unwrap().count(),
            1
        );

        match stage {
            Stage::ApplyLogin => {
                assert_eq!(fixture.calls().lines().count(), 3);
                assert_eq!(fs::read_to_string(&profile).unwrap(), "existing-token");
                assert_eq!(fs::read_to_string(&netrc).unwrap(), "old netrc");
                assert_eq!(fs::read_to_string(&nix_conf).unwrap(), "old nix.conf");
            }
            Stage::ApplyCache => {
                assert!(fs::read_to_string(&netrc).unwrap().contains(&token));
                assert_eq!(fs::read_to_string(&nix_conf).unwrap(), "old nix.conf");
            }
            Stage::Save => {
                assert!(fs::read_to_string(&netrc).unwrap().contains(&token));
                assert!(fs::read_to_string(&nix_conf).unwrap().ends_with("/main\n"));
            }
            _ => unreachable!(),
        }

        let (result, _) = fixture.run("success", &token, "main", TEST_TIMEOUT).await;
        assert_eq!(fixture.stored(), Some(result.unwrap()));
    }
}

#[tokio::test]
async fn reports_absent_or_non_executable_cli_and_metadata_preparation_failure() {
    let token = jwt(NOW + 3600, json!({ "main": { "r": 1 } }));
    let fixture = Fixture::new();
    for mode in [Some(0o600), None] {
        if let Some(mode) = mode {
            fs::set_permissions(&fixture.attic, fs::Permissions::from_mode(mode)).unwrap();
        } else {
            fs::remove_file(&fixture.attic).unwrap();
        }
        let (result, _) = fixture.run("success", &token, "main", TEST_TIMEOUT).await;
        assert_eq!(
            result,
            Err(Failure {
                stage: Stage::VerifyLogin,
                kind: FailureKind::Launch
            })
        );
        assert!(fixture.calls().is_empty());
        assert_eq!(fixture.stored(), None);
    }

    let fixture = Fixture::new();
    fs::create_dir_all(fixture.metadata_directory.parent().unwrap()).unwrap();
    fs::write(&fixture.metadata_directory, "not a directory").unwrap();
    let (result, journal) = fixture.run("success", &token, "main", TEST_TIMEOUT).await;
    assert_eq!(
        result,
        Err(Failure {
            stage: Stage::Prepare,
            kind: FailureKind::MetadataWrite
        })
    );
    assert!(journal.is_empty());
    assert!(fixture.calls().is_empty());
}
