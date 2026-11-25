use iced::Task;

use crate::{
    rebuild::{self, history},
    ui::screens::{Screen, build_history},
};

use super::{App, Message};

impl App {
    pub(super) fn can_activate_history(&self) -> bool {
        !self.is_busy()
            && self
                .history
                .as_ref()
                .is_some_and(|snapshot| !snapshot.unavailable)
            && self
                .rebuild_tools
                .missing(false, rebuild::Steps::ACTIVATE)
                .next()
                .is_none()
    }

    pub(super) fn refresh_history(&mut self) -> Task<Message> {
        if self.history_refreshing {
            return Task::none();
        }
        self.history_refreshing = true;
        self.history = None;
        self.history_generation += 1;
        let generation = self.history_generation;
        Task::perform(
            async {
                tokio::task::spawn_blocking(history::snapshot)
                    .await
                    .unwrap_or_else(|_| {
                        let mut snapshot = history::Snapshot::default();
                        snapshot.unavailable = true;
                        snapshot
                    })
            },
            move |snapshot| Message::HistoryLoaded(generation, snapshot),
        )
    }

    pub(super) fn history_loaded(&mut self, generation: u64, snapshot: history::Snapshot) {
        if generation != self.history_generation {
            return;
        }
        if !snapshot.unavailable
            && self
                .built
                .as_ref()
                .is_some_and(|build| !snapshot.contains_build(build))
        {
            self.built = None;
        }
        if let Screen::BuildHistory(state) = &mut self.screen {
            state.set_snapshot(&self.i18n, &snapshot);
        }
        self.history = Some(snapshot);
        self.history_refreshing = false;
    }

    pub(super) fn history_removed(&mut self, result: Result<(), rebuild::Error>) -> Task<Message> {
        if let Screen::BuildHistory(state) = &mut self.screen {
            state.removal_finished(result);
            let refresh = self.refresh_history();
            return Task::batch([refresh, self.finish_pending_close()]);
        }
        Task::none()
    }

    pub(super) fn update_history(&mut self, message: build_history::Message) -> Task<Message> {
        let can_activate = self.can_activate_history();
        let can_remove_generations = self
            .rebuild_tools
            .missing_for_generation_removal()
            .is_none();
        let Screen::BuildHistory(state) = &mut self.screen else {
            return Task::none();
        };
        match state.update(
            message,
            self.history.as_ref(),
            can_activate,
            can_remove_generations,
        ) {
            build_history::Action::None => Task::none(),
            build_history::Action::Close => {
                self.screen = Screen::Overview;
                Task::none()
            }
            build_history::Action::Refresh => self.refresh_history(),
            build_history::Action::Activate(target) => {
                self.screen = Screen::Overview;
                self.start_run(
                    rebuild::Request::Ready(target),
                    rebuild::Steps::ACTIVATE,
                    true,
                )
            }
            build_history::Action::Remove(marked) => {
                let runner = self.commands.clone();
                Task::perform(history::remove(runner, marked), Message::HistoryRemoved)
            }
        }
    }
}
