use colored::Colorize;

use crate::commands::security_report::{LineKind, security_guide, trust_summary};

pub fn print_trust_summary(compact: bool) {
    let summary = trust_summary(compact);

    if summary.compact {
        println!();
        println!("{}", "Trust model".bold().cyan());
    } else {
        println!("{}", "cloudcode security".bold().cyan());
        println!();
        println!("{}", "Trust model".bold());
    }

    for line in summary.lines {
        match line.kind {
            LineKind::Muted => println!("  {}", line.text.dimmed()),
            LineKind::Warning => println!("  {}", line.text.yellow()),
            LineKind::Plain => println!("  {}", line.text),
        }
    }

    if summary.compact {
        return;
    }

    let guide = security_guide();
    println!();
    println!("{}", "Revoke / Rotate".bold());
    for (idx, line) in guide.revoke_rotate.iter().enumerate() {
        println!("  {}. {}", idx + 1, line);
    }
    println!();
    println!("{}", "Verify".bold());
    for line in guide.verify {
        println!("  {}", line);
    }
}

pub async fn run() -> anyhow::Result<()> {
    print_trust_summary(false);
    Ok(())
}
