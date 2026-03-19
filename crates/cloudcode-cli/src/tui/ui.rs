use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use super::app::App;
use super::steps::{AppMode, InputFocus, ValidationStatus, WizardStep};

const BLUE: Color = Color::Cyan;
const GREEN: Color = Color::Green;
const YELLOW: Color = Color::Yellow;
const RED: Color = Color::Red;
const DIM: Color = Color::DarkGray;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Clear, area);

    match app.mode {
        AppMode::Wizard => draw_wizard(f, app, area),
        AppMode::Main => draw_main(f, app, area),
    }
}

// ── Wizard rendering ────────────────────────────────────────────────────

fn draw_wizard(f: &mut Frame, app: &App, area: Rect) {
    match app.step {
        WizardStep::Welcome => draw_welcome(f, app, area),
        WizardStep::Complete => draw_complete(f, app, area),
        _ => draw_step(f, app, area),
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

fn draw_welcome(f: &mut Frame, app: &App, area: Rect) {
    let box_height = if app.existing_config { 16 } else { 14 };
    let rect = centered_rect(50, box_height, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BLUE))
        .padding(Padding::uniform(1));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("         ☁  ", Style::default().fg(BLUE).bold()),
            Span::styled("cloudcode", Style::default().fg(BLUE).bold()),
        ]),
        Line::from(Span::styled(
            "         Persistent cloud AI sessions",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  This wizard will configure:",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    • Hetzner Cloud API token",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    • Claude authentication",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    • Telegram bot (optional)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "    • SSH keypair",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press Enter to begin.",
            Style::default().fg(BLUE).bold(),
        )),
    ];

    if app.existing_config {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ⚠ Existing config found. Will be overwritten.",
            Style::default().fg(YELLOW),
        )));
    }

    f.render_widget(Paragraph::new(Text::from(lines)), inner);

    // Help line below the box
    let help = Line::from(Span::styled(
        "  Enter: begin  ·  q: quit",
        Style::default().fg(DIM),
    ));
    let help_rect = Rect::new(rect.x, rect.y + rect.height, rect.width, 1);
    if help_rect.y < area.height {
        f.render_widget(Paragraph::new(help), help_rect);
    }
}

fn draw_step(f: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::vertical([
        Constraint::Length(2), // header
        Constraint::Length(1), // separator
        Constraint::Min(6),    // body
        Constraint::Length(1), // separator
        Constraint::Length(1), // footer
    ])
    .split(area);

    draw_header(f, app, layout[0]);
    draw_separator(f, layout[1]);

    match app.step {
        WizardStep::Hetzner => draw_hetzner(f, app, layout[2]),
        WizardStep::Provider => draw_provider(f, app, layout[2]),
        WizardStep::Claude => draw_claude(f, app, layout[2]),
        WizardStep::ClaudeApiKey => draw_claude_api_key(f, app, layout[2]),
        WizardStep::OAuthWarning => draw_oauth_warning(f, layout[2]),
        WizardStep::Codex => draw_codex(f, app, layout[2]),
        WizardStep::CodexApiKey => draw_codex_api_key(f, app, layout[2]),
        WizardStep::CodexOAuthWarning => draw_codex_oauth_warning(f, layout[2]),
        WizardStep::Telegram => draw_telegram(f, app, layout[2]),
        WizardStep::Generating => draw_generating(f, app, layout[2]),
        _ => {}
    }

    draw_separator(f, layout[3]);
    draw_wizard_footer(f, app, layout[4]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    if let Some(step_num) = app.step.step_number() {
        let total = WizardStep::total_steps();

        let dots: Vec<Span> = (1..=total)
            .map(|i| {
                if i < step_num {
                    Span::styled("●", Style::default().fg(GREEN))
                } else if i == step_num {
                    Span::styled("●", Style::default().fg(BLUE))
                } else {
                    Span::styled("○", Style::default().fg(DIM))
                }
            })
            .collect();

        let mut header_spans = vec![
            Span::styled("  Step ", Style::default().fg(BLUE)),
            Span::styled(format!("{step_num}"), Style::default().fg(BLUE).bold()),
            Span::styled(format!(" of {total}"), Style::default().fg(BLUE)),
            Span::styled(" · ", Style::default().fg(DIM)),
            Span::styled(app.step.label(), Style::default().fg(BLUE).bold()),
        ];

        let label_len = header_spans.iter().map(|s| s.width()).sum::<usize>();
        let dots_len = dots.len();
        let padding = area
            .width
            .saturating_sub(label_len as u16 + dots_len as u16 + 6);
        header_spans.push(Span::raw(" ".repeat(padding as usize)));
        header_spans.push(Span::styled("[", Style::default().fg(DIM)));
        header_spans.extend(dots);
        header_spans.push(Span::styled("]", Style::default().fg(DIM)));

        let line = Line::from(header_spans);
        let rect = Rect::new(area.x, area.y + 1, area.width, 1);
        f.render_widget(Paragraph::new(line), rect);
    }
}

fn draw_separator(f: &mut Frame, area: Rect) {
    let line = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(line, Style::default().fg(DIM)))),
        area,
    );
}

fn draw_wizard_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = match app.step {
        WizardStep::Hetzner => "Enter: submit  ·  Esc: back",
        WizardStep::Provider => "↑↓: select  ·  Enter: confirm  ·  Esc: back",
        WizardStep::Claude => "↑↓: select  ·  Enter: confirm  ·  Esc: back",
        WizardStep::ClaudeApiKey => "Enter: submit  ·  Esc: back",
        WizardStep::OAuthWarning => "Enter: continue  ·  Esc: back",
        WizardStep::Codex => "↑↓: select  ·  Enter: confirm  ·  Esc: back",
        WizardStep::CodexApiKey => "Enter: submit  ·  Esc: back",
        WizardStep::CodexOAuthWarning => "Enter: continue  ·  Esc: back",
        WizardStep::Telegram => {
            if app.telegram_enabled {
                "Tab: next field  ·  Enter: submit  ·  Esc: back"
            } else {
                "↑↓: select  ·  Enter: confirm  ·  Esc: back"
            }
        }
        WizardStep::Generating => "Please wait...",
        _ => "",
    };

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {help}"),
            Style::default().fg(DIM),
        ))),
        area,
    );
}

fn draw_hetzner(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let mut lines = vec![
        Line::from(Span::styled(
            "cloudcode needs a Hetzner Cloud API token to",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "provision and manage your VPS.",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Create one at console.hetzner.cloud",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "→ Security → API Tokens (Read & Write)",
            Style::default().fg(DIM),
        )),
        Line::from(""),
    ];

    let input_value = app.hetzner_input.value();
    let cursor_pos = app.hetzner_input.visual_cursor();

    lines.push(Line::from(vec![
        Span::styled("API Token: ", Style::default().fg(Color::White).bold()),
        Span::styled(input_value, Style::default().fg(Color::White)),
        Span::styled("▌", Style::default().fg(BLUE)),
    ]));

    match &app.hetzner_status {
        ValidationStatus::Idle => {}
        ValidationStatus::Validating => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("                    {} ", app.spinner_char()),
                    Style::default().fg(BLUE),
                ),
                Span::styled("Validating...", Style::default().fg(BLUE)),
            ]));
        }
        ValidationStatus::Success => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("                    ✓ ", Style::default().fg(GREEN).bold()),
                Span::styled("Token validated", Style::default().fg(GREEN)),
            ]));
        }
        ValidationStatus::Failed(err) => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("                    ✗ ", Style::default().fg(RED).bold()),
                Span::styled(err.as_str(), Style::default().fg(RED)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(Text::from(lines)), body);

    let cursor_x = body.x + "API Token: ".len() as u16 + cursor_pos as u16;
    let cursor_y = body.y + 6;
    if cursor_y < area.y + area.height {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_provider(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let options = ["Claude (Anthropic)", "Codex (OpenAI)", "Both"];
    let descs = [
        "Claude Code — Anthropic's coding agent",
        "Codex CLI — OpenAI's coding agent",
        "Configure both, choose default later",
    ];

    let mut lines = vec![
        Line::from(Span::styled(
            "Which AI provider would you like to use?",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
    ];

    for (i, (option, desc)) in options.iter().zip(descs.iter()).enumerate() {
        let selected = i == app.provider_choice;
        let marker = if selected { "● " } else { "○ " };
        let prefix = if selected { "› " } else { "  " };
        let style = if selected {
            Style::default().fg(BLUE).bold()
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(marker, style),
            Span::styled(*option, style),
            Span::styled(format!(" — {}", desc), Style::default().fg(DIM)),
        ]));
    }

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_claude(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let api_marker = if app.auth_choice == 0 { "● " } else { "○ " };
    let oauth_marker = if app.auth_choice == 1 { "● " } else { "○ " };

    let api_style = if app.auth_choice == 0 {
        Style::default().fg(BLUE).bold()
    } else {
        Style::default().fg(Color::White)
    };
    let oauth_style = if app.auth_choice == 1 {
        Style::default().fg(BLUE).bold()
    } else {
        Style::default().fg(Color::White)
    };

    let api_prefix = if app.auth_choice == 0 { "› " } else { "  " };
    let oauth_prefix = if app.auth_choice == 1 { "› " } else { "  " };

    let lines = vec![
        Line::from(Span::styled(
            "How would you like to authenticate?",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(api_prefix, api_style),
            Span::styled(api_marker, api_style),
            Span::styled("API Key", api_style),
            Span::styled(
                " — paste from console.anthropic.com",
                Style::default().fg(DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled(oauth_prefix, oauth_style),
            Span::styled(oauth_marker, oauth_style),
            Span::styled("OAuth", oauth_style),
            Span::styled(
                "   — log in on VPS after provisioning",
                Style::default().fg(DIM),
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_claude_api_key(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let input_value = app.api_key_input.value();
    let masked: String = "*".repeat(input_value.len());
    let cursor_pos = app.api_key_input.visual_cursor();

    let lines = vec![
        Line::from(Span::styled(
            "Enter your Anthropic API key:",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "Get one at console.anthropic.com/settings/keys",
            Style::default().fg(DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("API Key: ", Style::default().fg(Color::White).bold()),
            Span::styled(&masked, Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(BLUE)),
        ]),
    ];

    f.render_widget(Paragraph::new(Text::from(lines)), body);

    let cursor_x = body.x + "API Key: ".len() as u16 + cursor_pos as u16;
    let cursor_y = body.y + 3;
    if cursor_y < area.y + area.height {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_oauth_warning(f: &mut Frame, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let lines = vec![
        Line::from(Span::styled(
            "⚠  After provisioning, you'll need to log in",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "manually from the CLI:",
            Style::default().fg(YELLOW),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "1. /spawn (or cloudcode spawn)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "2. /open <session> (or cloudcode open <session>)",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "3. Claude will display a login URL",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "4. Highlight and copy the URL with your mouse",
            Style::default().fg(Color::White),
        )),
        Line::from(vec![
            Span::styled("   ⚠  Do NOT press 'c'", Style::default().fg(YELLOW).bold()),
            Span::styled(" — that copies to the", Style::default().fg(YELLOW)),
        ]),
        Line::from(Span::styled(
            "   VPS clipboard, not your local machine",
            Style::default().fg(YELLOW),
        )),
        Line::from(Span::styled(
            "5. Paste the URL in your local browser",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "6. Complete the login flow",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Telegram will not work until you complete this login.",
            Style::default().fg(YELLOW),
        )),
    ];

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_codex(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let api_marker = if app.codex_auth_choice == 0 { "● " } else { "○ " };
    let oauth_marker = if app.codex_auth_choice == 1 { "● " } else { "○ " };
    let api_style = if app.codex_auth_choice == 0 { Style::default().fg(BLUE).bold() } else { Style::default().fg(Color::White) };
    let oauth_style = if app.codex_auth_choice == 1 { Style::default().fg(BLUE).bold() } else { Style::default().fg(Color::White) };
    let api_prefix = if app.codex_auth_choice == 0 { "› " } else { "  " };
    let oauth_prefix = if app.codex_auth_choice == 1 { "› " } else { "  " };

    let lines = vec![
        Line::from(Span::styled("How would you like to authenticate with Codex?", Style::default().fg(Color::White))),
        Line::from(""),
        Line::from(vec![
            Span::styled(api_prefix, api_style),
            Span::styled(api_marker, api_style),
            Span::styled("API Key", api_style),
            Span::styled(" — paste from platform.openai.com", Style::default().fg(DIM)),
        ]),
        Line::from(vec![
            Span::styled(oauth_prefix, oauth_style),
            Span::styled(oauth_marker, oauth_style),
            Span::styled("Device Auth", oauth_style),
            Span::styled(" — log in on VPS via device code", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_codex_api_key(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let input_value = app.codex_api_key_input.value();
    let masked: String = "*".repeat(input_value.len());
    let cursor_pos = app.codex_api_key_input.visual_cursor();

    let lines = vec![
        Line::from(Span::styled("Enter your OpenAI API key:", Style::default().fg(Color::White))),
        Line::from(Span::styled("Get one at platform.openai.com/api-keys", Style::default().fg(DIM))),
        Line::from(""),
        Line::from(vec![
            Span::styled("API Key: ", Style::default().fg(Color::White).bold()),
            Span::styled(&masked, Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(BLUE)),
        ]),
    ];

    f.render_widget(Paragraph::new(Text::from(lines)), body);

    let cursor_x = body.x + "API Key: ".len() as u16 + cursor_pos as u16;
    let cursor_y = body.y + 3;
    if cursor_y < area.y + area.height {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_codex_oauth_warning(f: &mut Frame, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let lines = vec![
        Line::from(Span::styled("⚠  After provisioning, you'll need to log in", Style::default().fg(YELLOW))),
        Line::from(Span::styled("to Codex from the CLI:", Style::default().fg(YELLOW))),
        Line::from(""),
        Line::from(Span::styled("1. /spawn (or cloudcode spawn)", Style::default().fg(Color::White))),
        Line::from(Span::styled("2. /open <session> (or cloudcode open <session>)", Style::default().fg(Color::White))),
        Line::from(Span::styled("3. Select 'Device code' when Codex prompts", Style::default().fg(Color::White))),
        Line::from(Span::styled("4. Visit the URL in your local browser to authorize", Style::default().fg(Color::White))),
        Line::from(""),
        Line::from(Span::styled("⚠  Do NOT use the browser/localhost option", Style::default().fg(YELLOW).bold())),
        Line::from(Span::styled("   (it redirects to localhost which won't work on the VPS)", Style::default().fg(YELLOW))),
        Line::from(""),
        Line::from(Span::styled("Telegram will not work until you complete this login.", Style::default().fg(YELLOW))),
    ];

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_telegram(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let mut lines = vec![
        Line::from(Span::styled(
            "Set up a Telegram bot to chat with Claude",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "from your phone?",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
    ];

    if !app.telegram_enabled {
        let yes_marker = if app.telegram_choice == 0 {
            "● "
        } else {
            "○ "
        };
        let no_marker = if app.telegram_choice == 1 {
            "● "
        } else {
            "○ "
        };
        let yes_style = if app.telegram_choice == 0 {
            Style::default().fg(BLUE).bold()
        } else {
            Style::default().fg(Color::White)
        };
        let no_style = if app.telegram_choice == 1 {
            Style::default().fg(BLUE).bold()
        } else {
            Style::default().fg(Color::White)
        };
        let yes_prefix = if app.telegram_choice == 0 {
            "› "
        } else {
            "  "
        };
        let no_prefix = if app.telegram_choice == 1 {
            "› "
        } else {
            "  "
        };

        lines.push(Line::from(vec![
            Span::styled(yes_prefix, yes_style),
            Span::styled(yes_marker, yes_style),
            Span::styled("Yes", yes_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled(no_prefix, no_style),
            Span::styled(no_marker, no_style),
            Span::styled("No, skip", no_style),
        ]));
    } else {
        let token_val = app.telegram_token_input.value();
        let id_val = app.telegram_id_input.value();

        let token_active = app.telegram_focus == InputFocus::Primary;
        let id_active = app.telegram_focus == InputFocus::Secondary;

        let token_label_style = if token_active {
            Style::default().fg(BLUE).bold()
        } else {
            Style::default().fg(Color::White).bold()
        };
        let id_label_style = if id_active {
            Style::default().fg(BLUE).bold()
        } else {
            Style::default().fg(Color::White).bold()
        };

        lines.push(Line::from(vec![
            Span::styled("Bot Token: ", token_label_style),
            Span::styled(token_val, Style::default().fg(Color::White)),
            if token_active {
                Span::styled("▌", Style::default().fg(BLUE))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::from(vec![
            Span::styled("Owner ID:  ", id_label_style),
            Span::styled(id_val, Style::default().fg(Color::White)),
            if id_active {
                Span::styled("▌", Style::default().fg(BLUE))
            } else {
                Span::raw("")
            },
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Create a bot via @BotFather on Telegram.",
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            "Get your ID from @userinfobot.",
            Style::default().fg(DIM),
        )));

        let (cursor_x, cursor_y) = match app.telegram_focus {
            InputFocus::Primary => (
                body.x
                    + "Bot Token: ".len() as u16
                    + app.telegram_token_input.visual_cursor() as u16,
                body.y + 3,
            ),
            InputFocus::Secondary => (
                body.x + "Owner ID:  ".len() as u16 + app.telegram_id_input.visual_cursor() as u16,
                body.y + 4,
            ),
        };
        if cursor_y < area.y + area.height {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_generating(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    let ssh_line = if app.gen_ssh_done || app.ssh_key_exists {
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(GREEN).bold()),
            Span::styled(
                if app.ssh_key_exists && !app.gen_ssh_done {
                    "SSH keypair exists"
                } else {
                    "SSH keypair generated"
                },
                Style::default().fg(GREEN),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} ", app.spinner_char()),
                Style::default().fg(BLUE),
            ),
            Span::styled(
                "Generating SSH keypair...",
                Style::default().fg(Color::White),
            ),
        ])
    };

    let config_line = if app.gen_config_done {
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(GREEN).bold()),
            Span::styled("Configuration saved", Style::default().fg(GREEN)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} ", app.spinner_char()),
                Style::default().fg(BLUE),
            ),
            Span::styled("Saving configuration...", Style::default().fg(Color::White)),
        ])
    };

    let lines = vec![ssh_line, config_line];
    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

fn draw_complete(f: &mut Frame, app: &App, area: Rect) {
    let box_height = if app.is_oauth() { 18 } else { 14 };
    let rect = centered_rect(50, box_height, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .padding(Padding::uniform(1));

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("          ✓  ", Style::default().fg(GREEN).bold()),
            Span::styled("Setup complete!", Style::default().fg(GREEN).bold()),
        ]),
        Line::from(""),
    ];

    if let Some(ref h) = app.config.hetzner {
        lines.push(Line::from(vec![
            Span::styled("  Hetzner:   ", Style::default().fg(Color::White).bold()),
            Span::styled(App::mask_secret(&h.api_token), Style::default().fg(DIM)),
            Span::styled("            ✓", Style::default().fg(GREEN)),
        ]));
    }

    if let Some(ref c) = app.config.claude {
        let auth_display = if c.uses_api_key() {
            format!(
                "{} ({})",
                c.auth_display_name(),
                c.api_key
                    .as_deref()
                    .map(App::mask_secret)
                    .unwrap_or_default()
            )
        } else {
            c.auth_display_name().to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("  Claude:    ", Style::default().fg(Color::White).bold()),
            Span::styled(auth_display, Style::default().fg(DIM)),
        ]));
    }

    if let Some(ref t) = app.config.telegram {
        lines.push(Line::from(vec![
            Span::styled("  Telegram:  ", Style::default().fg(Color::White).bold()),
            Span::styled(App::mask_secret(&t.bot_token), Style::default().fg(DIM)),
            Span::styled("       ✓", Style::default().fg(GREEN)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Telegram:  ", Style::default().fg(Color::White).bold()),
            Span::styled("skipped", Style::default().fg(DIM)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  SSH Key:   ", Style::default().fg(Color::White).bold()),
        Span::styled("~/.cloudcode/id_ed25519", Style::default().fg(DIM)),
        Span::styled("  ✓", Style::default().fg(GREEN)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press Enter to continue to cloudcode.",
        Style::default().fg(BLUE).bold(),
    )));

    if app.is_oauth() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Run /up, then /spawn + /open to complete OAuth login.",
            Style::default().fg(YELLOW),
        )));
    }

    f.render_widget(Paragraph::new(Text::from(lines)), inner);

    let help = Line::from(Span::styled("  Enter: continue", Style::default().fg(DIM)));
    let help_rect = Rect::new(rect.x, rect.y + rect.height, rect.width, 1);
    if help_rect.y < area.height {
        f.render_widget(Paragraph::new(help), help_rect);
    }
}

// ── Main view rendering ─────────────────────────────────────────────────

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::vertical([
        Constraint::Length(2), // header
        Constraint::Length(1), // separator
        Constraint::Min(10),   // body
        Constraint::Length(1), // separator
        Constraint::Length(1), // status / error line
        Constraint::Length(1), // hint line
        Constraint::Length(1), // input line
    ])
    .split(area);

    // Header with VPS status
    let vps_status_span = if app.vps_state.is_provisioned() {
        let ip = app.vps_state.server_ip.as_deref().unwrap_or("unknown");
        let status = app.vps_state.status_name().unwrap_or("unknown");
        Span::styled(
            format!("  VPS: {status} ({ip})"),
            Style::default().fg(GREEN),
        )
    } else {
        Span::styled("  VPS: not provisioned", Style::default().fg(DIM))
    };

    let mut header_spans = vec![
        Span::styled("  ☁ ", Style::default().fg(BLUE).bold()),
        Span::styled("cloudcode", Style::default().fg(BLUE).bold()),
    ];
    // Right-align VPS status
    let left_len: usize = header_spans.iter().map(|s| s.width()).sum();
    let right_len = vps_status_span.width();
    let pad = (layout[0].width as usize).saturating_sub(left_len + right_len + 2);
    header_spans.push(Span::raw(" ".repeat(pad)));
    header_spans.push(vps_status_span);

    let header = Line::from(header_spans);
    let header_rect = Rect::new(layout[0].x, layout[0].y + 1, layout[0].width, 1);
    f.render_widget(Paragraph::new(header), header_rect);

    draw_separator(f, layout[1]);

    // Body: server type picker, help reference, or console log
    if app.server_type_picker.is_some() {
        draw_server_type_picker(f, app, layout[2]);
    } else if app.show_help && app.log_lines.is_empty() && !app.is_command_running() {
        draw_command_reference(f, app, layout[2]);
    } else {
        draw_console_log(f, app, layout[2]);
    }

    draw_separator(f, layout[3]);

    // Status / error line
    if let Some(ref err) = app.error_message {
        let line = Line::from(vec![
            Span::styled("  ✗ ", Style::default().fg(RED).bold()),
            Span::styled(err.as_str(), Style::default().fg(RED)),
        ]);
        f.render_widget(Paragraph::new(line), layout[4]);
    } else if app.is_command_running() {
        let line = Line::from(vec![
            Span::styled(
                format!("  {} ", app.spinner_char()),
                Style::default().fg(BLUE),
            ),
            Span::styled("running...", Style::default().fg(DIM)),
        ]);
        f.render_widget(Paragraph::new(line), layout[4]);
    }

    // Hint line
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Type /help for a list of commands",
            Style::default().fg(DIM),
        ))),
        layout[5],
    );

    // Input line
    let input_value = app.command_input.value();
    let cursor_pos = app.command_input.visual_cursor();

    let (input_line, prompt_len) = if let Some(ref prompt) = app.inline_prompt {
        let label = format!("  {} ", prompt.label);
        let len = label.len();
        (
            Line::from(vec![
                Span::styled(label, Style::default().fg(YELLOW)),
                Span::styled(input_value, Style::default().fg(Color::White)),
                Span::styled("▌", Style::default().fg(BLUE)),
            ]),
            len,
        )
    } else {
        (
            Line::from(vec![
                Span::styled("  > ", Style::default().fg(BLUE).bold()),
                Span::styled("/", Style::default().fg(DIM)),
                Span::styled(input_value, Style::default().fg(Color::White)),
                Span::styled("▌", Style::default().fg(BLUE)),
            ]),
            "  > /".len(),
        )
    };
    f.render_widget(Paragraph::new(input_line), layout[6]);

    let cursor_x = layout[6].x + prompt_len as u16 + cursor_pos as u16;
    let cursor_y = layout[6].y;
    if cursor_y < area.y + area.height {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_console_log(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 0));
    let available = body.height as usize;

    let mut lines: Vec<Line> = Vec::new();

    // History entries
    for entry in &app.history {
        let status_span = if entry.exit_ok {
            Span::styled("  ✓", Style::default().fg(GREEN).bold())
        } else {
            Span::styled("  ✗", Style::default().fg(RED).bold())
        };
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(DIM)),
            Span::styled(entry.command.as_str(), Style::default().fg(DIM).bold()),
            status_span,
        ]));
        for log_line in &entry.lines {
            let style = if log_line.is_error {
                Style::default().fg(RED).dim()
            } else {
                Style::default().fg(DIM)
            };
            lines.push(Line::from(Span::styled(&log_line.text, style)));
        }
        lines.push(Line::from(""));
    }

    // Current command header
    if let Some(ref cmd_name) = app.running_command {
        let status_span = if app.command_done {
            Span::styled("  ✓", Style::default().fg(GREEN).bold())
        } else {
            Span::styled(
                format!("  {}", app.spinner_char()),
                Style::default().fg(BLUE),
            )
        };
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(BLUE)),
            Span::styled(cmd_name.as_str(), Style::default().fg(BLUE).bold()),
            status_span,
        ]));
        lines.push(Line::from(""));
    }

    // Current output lines
    for log_line in &app.log_lines {
        let clean = strip_ansi(&log_line.text);
        let style = if log_line.is_error {
            Style::default().fg(RED)
        } else {
            Style::default().fg(BLUE)
        };
        lines.push(Line::from(Span::styled(clean, style)));
    }

    // Scrolling: show last N lines, offset by log_scroll
    let total = lines.len();
    let max_scroll = total.saturating_sub(available);
    let scroll = app.log_scroll.min(max_scroll);
    let skip = total.saturating_sub(available + scroll);
    let visible: Vec<Line> = lines.into_iter().skip(skip).take(available).collect();

    f.render_widget(Paragraph::new(Text::from(visible)), body);
}

fn draw_server_type_picker(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));
    let mut lines: Vec<Line> = Vec::new();

    let picker = app.server_type_picker.as_ref().unwrap();

    lines.push(Line::from(vec![
        Span::styled(
            "Select a server type for provisioning",
            Style::default().fg(Color::White).bold(),
        ),
        Span::styled(
            format!("  (location: {})", picker.location),
            Style::default().fg(DIM),
        ),
    ]));
    lines.push(Line::from(""));

    if picker.loading {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", app.spinner_char()),
                Style::default().fg(BLUE),
            ),
            Span::styled(
                "Fetching available server types...",
                Style::default().fg(Color::White),
            ),
        ]));
    } else if picker.types.is_empty() {
        lines.push(Line::from(Span::styled(
            "No server types available.",
            Style::default().fg(RED),
        )));
    } else {
        // Header
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<12} {:<6} {:<8} {:<8} {:<12} {}",
                "Name", "CPUs", "RAM", "Disk", "Cost/mo", "Status"
            ),
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", "─".repeat(62)),
            Style::default().fg(DIM),
        )));

        for (i, st) in picker.types.iter().enumerate() {
            let is_selected = i == picker.selected;
            let available_here = st.available_locations.contains(&picker.location);

            let prefix = if is_selected { "› " } else { "  " };

            let cost_str = if let Some(ref price) = st.monthly_price {
                // Use real API price for the configured location
                if let Ok(val) = price.parse::<f64>() {
                    format!("${:.2}", val)
                } else {
                    price.clone()
                }
            } else {
                "—".to_string()
            };

            let status = if available_here {
                "✓ available"
            } else {
                "✗ unavailable"
            };

            let style = if !available_here {
                Style::default().fg(DIM)
            } else if is_selected {
                Style::default().fg(BLUE).bold()
            } else {
                Style::default().fg(Color::White)
            };

            let status_style = if !available_here {
                Style::default().fg(RED).dim()
            } else if is_selected {
                Style::default().fg(GREEN).bold()
            } else {
                Style::default().fg(GREEN)
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{}{:<12} {:<6} {:<8} {:<8} {:<12} ",
                        prefix,
                        st.name,
                        format!("{}x", st.cores),
                        format!("{:.0} GB", st.memory),
                        format!("{} GB", st.disk),
                        cost_str,
                    ),
                    style,
                ),
                Span::styled(status, status_style),
                Span::styled(format!("  {}", st.description), Style::default().fg(DIM)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ↑↓: select  ·  Enter: confirm  ·  Esc: cancel",
            Style::default().fg(DIM),
        )));
    }

    let available = body.height as usize;
    // Scroll if list is longer than available space
    let total = lines.len();
    if total > available {
        let skip = total.saturating_sub(available);
        let visible: Vec<Line> = lines.into_iter().skip(skip).take(available).collect();
        f.render_widget(Paragraph::new(Text::from(visible)), body);
    } else {
        f.render_widget(Paragraph::new(Text::from(lines)), body);
    }
}

fn draw_command_reference(f: &mut Frame, app: &App, area: Rect) {
    let body = area.inner(Margin::new(2, 1));

    // (slash_cmd, args, description, cli_equivalent)
    let commands: Vec<(&str, &str, &str, &str)> = vec![
        ("VPS & Sessions", "", "", ""),
        ("  /up", "", "Provision VPS", "cloudcode up"),
        ("  /down", "", "Destroy VPS", "cloudcode down"),
        (
            "  /status",
            "",
            "Show VPS & session status",
            "cloudcode status",
        ),
        (
            "  /spawn",
            " [name]",
            "Create a session",
            "cloudcode spawn",
        ),
        ("  /list", "", "List active sessions", "cloudcode list"),
        (
            "  /open",
            " <session>",
            "Open session interactively",
            "cloudcode open <s>",
        ),
        (
            "  /send",
            " <s> <msg>",
            "Send message to session",
            "cloudcode send <s> <m>",
        ),
        (
            "  /kill",
            " <session>",
            "Kill a session",
            "cloudcode kill <s>",
        ),
        ("", "", "", ""),
        ("System", "", "", ""),
        (
            "  /provider",
            " [name]",
            "Show/switch provider",
            "cloudcode provider",
        ),
        (
            "  /restart",
            "",
            "Restart daemon on VPS",
            "cloudcode restart",
        ),
        (
            "  /logs",
            " [target]",
            "View logs (setup/daemon)",
            "cloudcode logs",
        ),
        ("  /ssh", " [cmd]", "SSH to the VPS", "cloudcode ssh"),
        ("", "", "", ""),
        ("Other", "", "", ""),
        ("  /init", "", "Re-run setup wizard", "cloudcode init"),
        ("  /help", "", "Show this reference", ""),
    ];

    // VPS status banner
    let mut lines: Vec<Line> = Vec::new();
    if app.vps_state.is_provisioned() {
        let ip = app.vps_state.server_ip.as_deref().unwrap_or("unknown");
        let status = app.vps_state.status_name().unwrap_or("unknown");
        lines.push(Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(GREEN).bold()),
            Span::styled(
                format!("Active VPS: {status} at {ip}"),
                Style::default().fg(GREEN),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            "    Run /status (or cloudcode status) for details, /spawn to create a session.",
            Style::default().fg(DIM),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  ○ ", Style::default().fg(YELLOW)),
            Span::styled("No active VPS.", Style::default().fg(YELLOW)),
        ]));
        lines.push(Line::from(Span::styled(
            "    Run /up (or cloudcode up) to provision one, or /init to reconfigure.",
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(""));

    let cmd_lines: Vec<Line> = commands
        .iter()
        .map(|(cmd, args, desc, cli)| {
            if desc.is_empty() && args.is_empty() {
                if cmd.is_empty() {
                    Line::from("")
                } else {
                    Line::from(Span::styled(*cmd, Style::default().fg(Color::White).bold()))
                }
            } else {
                let cmd_width = 12;
                let args_width = 12;
                let padded_cmd = format!("{:<width$}", cmd, width = cmd_width);
                let padded_args = format!("{:<width$}", args, width = args_width);
                let mut spans = vec![
                    Span::styled(padded_cmd, Style::default().fg(BLUE)),
                    Span::styled(padded_args, Style::default().fg(DIM)),
                    Span::styled(*desc, Style::default().fg(Color::White)),
                ];
                if !cli.is_empty() {
                    spans.push(Span::styled(format!("  ({cli})"), Style::default().fg(DIM)));
                }
                Line::from(spans)
            }
        })
        .collect();
    lines.extend(cmd_lines);

    f.render_widget(Paragraph::new(Text::from(lines)), body);
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC [ ... m sequences
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
