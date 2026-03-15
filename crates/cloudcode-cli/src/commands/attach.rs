use anyhow::Result;
use colored::Colorize;

pub async fn run(_session: String) -> Result<()> {
    println!(
        "{}",
        "Interactive attach not yet implemented (Phase 3).".yellow()
    );
    println!("Use `cloudcode send <session> <message>` to interact.");
    Ok(())
}
