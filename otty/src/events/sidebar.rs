use iced::Task;

use super::AppEvent;
use crate::app::App;
use crate::widgets::quick_launch::{QuickLaunchEvent, QuickLaunchIntent};
use crate::widgets::sidebar::{SidebarEffect, SidebarEvent};
use crate::widgets::tabs::{TabsEvent, TabsIntent};

pub(crate) fn handle(app: &mut App, event: SidebarEvent) -> Task<AppEvent> {
    match event {
        SidebarEvent::Intent(event) => {
            app.widgets.sidebar.reduce(event).map(AppEvent::Sidebar)
        },
        SidebarEvent::Effect(effect) => handle_effect(app, effect),
    }
}

fn handle_effect(app: &App, event: SidebarEffect) -> Task<AppEvent> {
    use SidebarEffect::*;

    match event {
        SyncTerminalGridSizes => Task::done(AppEvent::SyncTerminalGridSizes),
        OpenSettingsTab => Task::done(AppEvent::Tabs(TabsEvent::Intent(
            TabsIntent::OpenSettingsTab,
        ))),
        OpenTerminalTab => open_terminal_tab_task(app),
        QuickLaunchHeaderCreateCommand => Task::done(AppEvent::QuickLaunch(
            QuickLaunchEvent::Intent(QuickLaunchIntent::HeaderCreateCommand),
        )),
        QuickLaunchHeaderCreateFolder => Task::done(AppEvent::QuickLaunch(
            QuickLaunchEvent::Intent(QuickLaunchIntent::HeaderCreateFolder),
        )),
        QuickLaunchResetInteractionState => Task::done(AppEvent::QuickLaunch(
            QuickLaunchEvent::Intent(QuickLaunchIntent::ResetInteractionState),
        )),
    }
}

#[cfg(not(windows))]
fn open_terminal_tab_task(app: &App) -> Task<AppEvent> {
    Task::done(AppEvent::Tabs(TabsEvent::Intent(
        TabsIntent::OpenTerminalTab {
            title: app.shell_session.name().to_string(),
        },
    )))
}

/// Local sessions are unsupported on Windows; route new-tab requests to the
/// quick launch wizard pre-selected to SSH instead.
#[cfg(windows)]
fn open_terminal_tab_task(_app: &App) -> Task<AppEvent> {
    use crate::domain::quick_launch::WizardTabInit;
    use crate::widgets::quick_launch::types::QuickLaunchType;

    Task::done(AppEvent::Tabs(TabsEvent::Intent(
        TabsIntent::OpenWizardTab {
            title: String::from("New SSH Connection"),
            init: WizardTabInit::Create {
                parent_path: Vec::new(),
                command_type: QuickLaunchType::Ssh,
            },
        },
    )))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::widgets::sidebar::SidebarEffect;

    #[test]
    fn given_windows_when_open_terminal_tab_then_ssh_wizard_opens() {
        let (app, _) = App::new();

        let task = handle_effect(&app, SidebarEffect::OpenTerminalTab);

        assert_eq!(task.units(), 1);
    }
}
