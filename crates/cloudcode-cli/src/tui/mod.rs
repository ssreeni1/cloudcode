mod app;
mod steps;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use app::{App, SlashCommand};

/// What the event loop wants to do next.
enum LoopAction {
    Quit,
    /// Suspend TUI for an interactive command (needs real terminal).
    SuspendForCommand(SlashCommand),
}

/// Launch the persistent TUI.
///
/// If `force_wizard` is true, always show the onboarding wizard first.
/// Otherwise, skip directly to main view if config already exists.
pub async fn run_tui(force_wizard: bool) -> Result<()> {
    // Bail early if not running in a real terminal
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        anyhow::bail!(
            "cloudcode TUI requires a terminal. Run `cloudcode --help` for CLI usage."
        );
    }

    check_required_tools()?;

    let mut app = App::new(force_wizard)?;

    loop {
        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        terminal.clear()?;

        let action = run_event_loop(&mut terminal, &mut app).await?;

        drop(terminal);
        disable_raw_mode()?;
        io::stdout().execute(LeaveAlternateScreen)?;

        match action {
            LoopAction::Quit => break,
            LoopAction::SuspendForCommand(cmd) => {
                execute_interactive(cmd).await;
                println!("\nPress Enter to return to cloudcode...");
                let _ = io::stdin().read_line(&mut String::new());
            }
        }
    }

    Ok(())
}

/// The core event loop. Returns when the user quits or an interactive
/// command needs the real terminal.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<LoopAction> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // Check if an interactive command needs TUI suspension
        if let Some(cmd) = app.pending_command.take() {
            return Ok(LoopAction::SuspendForCommand(cmd));
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        // Poll wizard validation events
        while let Ok(event) = app.validation_rx.try_recv() {
            app.handle_validation(event);
        }

        // Poll command log events
        while let Ok(event) = app.log_rx.try_recv() {
            app.handle_log_event(event);
        }

        app.tick();

        if app.should_quit {
            return Ok(LoopAction::Quit);
        }
    }
}

/// Execute an interactive command with TUI suspended.
async fn execute_interactive(cmd: SlashCommand) {
    let result = match cmd {
        SlashCommand::Open(session) => crate::commands::attach::run(session).await,
        SlashCommand::Ssh(command) => crate::commands::ssh_cmd::run(command).await,
        SlashCommand::Down => crate::commands::down::run(false).await,
        _ => Ok(()),
    };

    if let Err(e) = result {
        eprintln!("\nError: {e:?}");
    }
}

fn check_required_tools() -> Result<()> {
    let tools = ["ssh", "rsync", "ssh-keygen"];
    let mut missing = Vec::new();

    for tool in &tools {
        let result = std::process::Command::new("which").arg(tool).output();
        match result {
            Ok(output) if output.status.success() => {}
            _ => missing.push(*tool),
        }
    }

    if !missing.is_empty() {
        anyhow::bail!(
            "Required tool(s) not found: {}. Please install them before running cloudcode.",
            missing.join(", ")
        );
    }

    Ok(())
}
