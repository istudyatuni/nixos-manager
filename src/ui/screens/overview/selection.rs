use std::borrow::Cow;

use fluent_bundle::FluentArgs;
use iced::{
    Color, Element, Font,
    widget::text_editor::{Action, Content},
};

use super::{DownloadStatus, Message, download_status_parts, journal};
use crate::{
    command_runner::LogLine, git::WorkingCopy, i18n::I18n, rebuild::Run,
    ui::components::selectable_text,
};

const REPOSITORY_STATUS_SIZE: u32 = 14;

#[derive(Clone, Copy)]
pub enum Target {
    WorkingCopy,
    PendingCheckout,
    RebuildError,
    DownloadError,
    DownloadDetail,
}

pub struct State {
    pub actions_menu_open: bool,
    pub journal_expanded: bool,
    pub(super) journal: journal::Output,
    pub(super) working_copy: selectable_text::Label,
    pub(super) pending_checkout: Option<selectable_text::Label>,
    pub rebuild_error: Content,
    pub download_error: Content,
    pub download_detail: Content,
}

impl Default for State {
    fn default() -> Self {
        Self {
            actions_menu_open: false,
            journal_expanded: false,
            journal: journal::Output::default(),
            working_copy: selectable_text::Label::new(REPOSITORY_STATUS_SIZE),
            pending_checkout: None,
            rebuild_error: Content::new(),
            download_error: Content::new(),
            download_detail: Content::new(),
        }
    }
}

impl State {
    pub fn update_selection(&mut self, target: Target, action: Action) -> Option<String> {
        match target {
            Target::WorkingCopy => self.working_copy.perform(action),
            Target::PendingCheckout => self.pending_checkout.as_mut()?.perform(action),
            Target::RebuildError => selectable_text::perform(&mut self.rebuild_error, action),
            Target::DownloadError => selectable_text::perform(&mut self.download_error, action),
            Target::DownloadDetail => selectable_text::perform(&mut self.download_detail, action),
        }
    }

    pub fn set_repository_status(
        &mut self,
        i18n: &I18n,
        working_copy: &WorkingCopy,
        selected_branch: &str,
    ) {
        let branch = working_copy.branch.as_deref();
        let commit = working_copy.commit.as_deref();
        let selected_commit = working_copy.selected_commit.as_deref();
        self.working_copy.set_text(&repository_status(
            i18n,
            "overview-repository-working-copy",
            branch,
            commit,
            working_copy.has_local_changes,
        ));
        if branch != Some(selected_branch) || commit.is_none() || commit != selected_commit {
            self.pending_checkout
                .get_or_insert_with(|| selectable_text::Label::new(REPOSITORY_STATUS_SIZE))
                .set_text(&repository_status(
                    i18n,
                    "overview-repository-pending",
                    Some(selected_branch),
                    selected_commit,
                    false,
                ));
        } else {
            self.pending_checkout = None;
        }
    }

    pub fn has_journal(&self) -> bool {
        !self.journal.is_empty()
    }

    pub fn journal_text(&self) -> Option<String> {
        self.journal.text()
    }

    pub fn clear_journal(&mut self) {
        self.journal = journal::Output::default();
    }

    pub fn append_journal(&mut self, lines: impl IntoIterator<Item = LogLine>) {
        self.journal.append(lines);
    }

    pub fn set_rebuild_error(&mut self, i18n: &I18n, run: Option<&Run>) {
        self.rebuild_error = match run.filter(|run| journal::has_error(run)) {
            Some(run) => Content::with_text(&journal::rebuild_status(i18n, run).0),
            None => Content::new(),
        };
    }

    pub fn set_download_error(&mut self, i18n: &I18n, download: &DownloadStatus) {
        if matches!(download, DownloadStatus::Finished(Err(_)))
            && let Some((key, _, detail)) = download_status_parts(download)
        {
            self.download_error = Content::with_text(&i18n.text(key));
            self.download_detail = Content::with_text(detail.unwrap_or_default());
        } else {
            self.download_error = Content::new();
            self.download_detail = Content::new();
        }
    }
}

fn repository_status(
    i18n: &I18n,
    key: &str,
    branch: Option<&str>,
    commit: Option<&str>,
    dirty: bool,
) -> String {
    let mut args = FluentArgs::new();
    let state = if dirty {
        "dirty"
    } else if branch.is_none() && commit.is_none() {
        "missing"
    } else {
        "clean"
    };
    args.set("state", state);
    let branch = match branch {
        Some(name) => {
            args.set("name", name);
            i18n.with_args("overview-repository-branch", &args)
        }
        None => i18n.text("overview-repository-detached"),
    };
    args.set("branch", branch);
    args.set(
        "commit",
        commit.map_or_else(
            || i18n.text("overview-repository-no-commit").into(),
            |commit| Cow::Borrowed(commit.get(..12).unwrap_or(commit)),
        ),
    );
    // Fluent isolates interpolations. Keep copied branch names and commit hashes clean.
    i18n.with_args(key, &args)
        .replace(['\u{2068}', '\u{2069}'], "")
}

pub fn view(
    content: &Content,
    target: Target,
    font: Font,
    size: u32,
    color: Color,
) -> Element<'_, Message> {
    selectable_text::view(content, font, size, color, move |action| {
        Message::SelectText(target, action)
    })
}
