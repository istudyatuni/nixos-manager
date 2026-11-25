use std::sync::Arc;

use iced::{Task, futures::SinkExt, stream, window};
use tokio::sync::mpsc;

use crate::{rebuild, ui::screens::Screen};

use super::{App, Message};

impl App {
    pub(super) fn start_rebuild(&mut self) -> Task<Message> {
        let cached = self.current_build().cloned();
        let needs_build = cached.is_none();
        let steps = self.steps;
        if self.is_busy()
            || !matches!(self.screen, Screen::Overview)
            || !self.working_copy.matches_revision
            || (!needs_build && !steps.applies())
            || self
                .rebuild_tools
                .missing(needs_build, steps)
                .next()
                .is_some()
        {
            return Task::none();
        }
        let Some(config) = self.config.clone() else {
            return Task::none();
        };
        self.start_run(
            rebuild::Request::Repository {
                config: Box::new(config),
                steps,
                cached,
            },
            steps,
            needs_build,
        )
    }

    pub(super) fn start_run(
        &mut self,
        request: rebuild::Request,
        steps: rebuild::Steps,
        clear: bool,
    ) -> Task<Message> {
        let (interrupt_request, interrupt) = mpsc::channel(1);
        self.rebuild_interrupt = Some(interrupt_request);
        let ready = matches!(&request, rebuild::Request::Ready(_));
        self.rebuild = Some(rebuild::Run::new(steps, ready));
        self.history_generation += 1;
        self.history_refreshing = false;
        self.overview.journal_expanded = true;
        if clear {
            self.clear_journal();
        }
        self.overview.set_rebuild_error(&self.i18n, None);
        let mut runner = self.commands.clone();
        if let rebuild::Request::Repository { config, .. } = &request {
            self.invalidate_repository_check();
            runner = runner.with_secret(config.token.clone());
        }
        Task::run(
            stream::channel(16, async move |mut sender| {
                let outcome = rebuild::run(&runner, request, interrupt, |event| {
                    let mut sender = sender.clone();
                    async move {
                        let _ = sender.send(event).await;
                    }
                })
                .await;
                let _ = sender.send(rebuild::Event::Finished(outcome)).await;
            }),
            Message::Rebuild,
        )
    }

    pub(super) fn update_rebuild(&mut self, event: rebuild::Event) -> Task<Message> {
        if let Some(run) = self.rebuild.as_mut().filter(|run| run.is_running()) {
            match event {
                rebuild::Event::Inspected(status) => {
                    self.working_copy = status;
                    self.refresh_repository_status();
                }
                rebuild::Event::Started(step) => run.step = step,
                rebuild::Event::Built(build) => self.built = Some(build),
                rebuild::Event::InterruptFailed => {
                    run.interrupting = false;
                    if !matches!(&self.screen, Screen::CacheSettings(state) if state.busy()) {
                        self.pending_close = None;
                    }
                }
                rebuild::Event::Finished(outcome) => {
                    if outcome.is_ok() {
                        self.overview.journal_expanded = false;
                    }
                    if !run.ready && matches!(outcome, Err(rebuild::Error::BuildResult)) {
                        self.built = None;
                    }
                    run.outcome = Some(outcome);
                    self.rebuild_interrupt = None;
                    if !matches!(&self.screen, Screen::CacheSettings(state) if state.busy())
                        && let Some(id) = self.pending_close.take()
                    {
                        return window::close(id);
                    }
                    self.overview.set_rebuild_error(&self.i18n, Some(run));
                    let repository = !run.ready;
                    let history = self.refresh_history();
                    return if repository {
                        Task::batch([history, self.inspect_repository()])
                    } else {
                        history
                    };
                }
            }
        }
        Task::none()
    }

    pub(super) fn interrupt_rebuild(&mut self) {
        if let (Some(run), Some(interrupt)) = (&mut self.rebuild, &self.rebuild_interrupt)
            && run.is_running()
            && !run.interrupting
            && interrupt.try_send(()).is_ok()
        {
            run.interrupting = true;
        }
    }

    pub(super) fn current_build(&self) -> Option<&Arc<rebuild::Build>> {
        self.built
            .as_ref()
            .filter(|build| build.matches_worktree(&self.working_copy))
    }
}
