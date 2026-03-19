use anyhow::{Context, Result, bail};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::IsTerminal;
use std::time::Duration;

use crate::config::{AuthMethod, Config};
use crate::hetzner::client::HetznerClient;
use crate::hetzner::provisioner;
use crate::ssh::connection::wait_for_ssh;
use crate::ssh::health::{self, CloudInitStatus};
use crate::state::{VpsState, VpsStatus};

use super::{
    get_daemon_binary, install_daemon, target_triple_for_server_type, upload_binary, verify_daemon,
};

const TOTAL_STEPS: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentStage {
    GenerateCloudInit,
    CreateHetznerKey,
    ProvisionServer,
    WaitForSsh,
    WaitForCloudInit,
    VerifyInstallation,
    PrepareDaemonBinary,
    UploadDaemonBinary,
    InstallDaemonService,
    VerifyDaemon,
}

impl DeploymentStage {
    fn number(self) -> u8 {
        match self {
            Self::GenerateCloudInit => 1,
            Self::CreateHetznerKey => 2,
            Self::ProvisionServer => 3,
            Self::WaitForSsh => 4,
            Self::WaitForCloudInit => 5,
            Self::VerifyInstallation => 6,
            Self::PrepareDaemonBinary => 7,
            Self::UploadDaemonBinary => 8,
            Self::InstallDaemonService => 9,
            Self::VerifyDaemon => 10,
        }
    }

    fn label(self, server_type: &str, location: &str, target: &str) -> String {
        match self {
            Self::GenerateCloudInit => "Generating cloud-init config...".to_string(),
            Self::CreateHetznerKey => "Creating SSH key in Hetzner...".to_string(),
            Self::ProvisionServer => {
                format!("Provisioning server ({server_type} in {location})...")
            }
            Self::WaitForSsh => {
                "Waiting for SSH login readiness (cloud-init user setup)...".to_string()
            }
            Self::WaitForCloudInit => {
                "Waiting for cloud-init to complete (usually 3-5 min)...".to_string()
            }
            Self::VerifyInstallation => "Verifying installed software...".to_string(),
            Self::PrepareDaemonBinary => format!("Preparing daemon for {target}..."),
            Self::UploadDaemonBinary => "Uploading daemon binary to VPS...".to_string(),
            Self::InstallDaemonService => "Installing daemon service...".to_string(),
            Self::VerifyDaemon => "Verifying daemon is running...".to_string(),
        }
    }

    fn success_message(self, server_type: &str, location: &str, target: &str) -> String {
        match self {
            Self::GenerateCloudInit => "Generated cloud-init config".to_string(),
            Self::CreateHetznerKey => "Created SSH key in Hetzner".to_string(),
            Self::ProvisionServer => format!("Provisioned server ({server_type} in {location})"),
            Self::WaitForSsh => "SSH is reachable".to_string(),
            Self::WaitForCloudInit => "Cloud-init completed successfully".to_string(),
            Self::VerifyInstallation => "All software verified".to_string(),
            Self::PrepareDaemonBinary => format!("Daemon ready for {target}"),
            Self::UploadDaemonBinary => "Daemon binary uploaded".to_string(),
            Self::InstallDaemonService => "Daemon service installed".to_string(),
            Self::VerifyDaemon => "Daemon is running".to_string(),
        }
    }

    fn failure_message(self, server_type: &str, location: &str, target: &str) -> String {
        match self {
            Self::GenerateCloudInit => "Failed to generate cloud-init config".to_string(),
            Self::CreateHetznerKey => "Failed to create SSH key in Hetzner".to_string(),
            Self::ProvisionServer => {
                format!("Failed to provision server ({server_type} in {location})")
            }
            Self::WaitForSsh => "SSH is not reachable yet".to_string(),
            Self::WaitForCloudInit => "Cloud-init did not complete successfully".to_string(),
            Self::VerifyInstallation => "Software verification found issues".to_string(),
            Self::PrepareDaemonBinary => format!("Failed to prepare daemon for {target}"),
            Self::UploadDaemonBinary => "Failed to upload daemon binary".to_string(),
            Self::InstallDaemonService => "Failed to install daemon service".to_string(),
            Self::VerifyDaemon => "Daemon verification found issues".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeploymentPlan {
    stages: Vec<DeploymentStage>,
}

impl DeploymentPlan {
    pub fn for_mode(no_wait: bool) -> Self {
        let stages = if no_wait {
            vec![
                DeploymentStage::GenerateCloudInit,
                DeploymentStage::CreateHetznerKey,
                DeploymentStage::ProvisionServer,
            ]
        } else {
            vec![
                DeploymentStage::GenerateCloudInit,
                DeploymentStage::CreateHetznerKey,
                DeploymentStage::ProvisionServer,
                DeploymentStage::WaitForSsh,
                DeploymentStage::WaitForCloudInit,
                DeploymentStage::VerifyInstallation,
                DeploymentStage::PrepareDaemonBinary,
                DeploymentStage::UploadDaemonBinary,
                DeploymentStage::InstallDaemonService,
                DeploymentStage::VerifyDaemon,
            ]
        };
        Self { stages }
    }

    pub fn total_steps(&self) -> u8 {
        TOTAL_STEPS
    }

    pub fn stages(&self) -> &[DeploymentStage] {
        &self.stages
    }
}

struct ConsoleReporter {
    tty: bool,
    total_steps: u8,
}

impl ConsoleReporter {
    fn new(total_steps: u8) -> Self {
        Self {
            tty: std::io::stdout().is_terminal(),
            total_steps,
        }
    }

    fn start(
        &self,
        stage: DeploymentStage,
        server_type: &str,
        location: &str,
        target: &str,
    ) -> ProgressBar {
        let msg = stage.label(server_type, location, target);
        let step = stage.number();
        if !self.tty {
            println!("  ... [{step}/{}] {msg}", self.total_steps);
            ProgressBar::hidden()
        } else {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template(&format!(
                        "  {{spinner:.green}} [{step}/{}] {{msg}}",
                        self.total_steps
                    ))
                    .expect("invalid template"),
            );
            pb.set_message(msg);
            pb.enable_steady_tick(Duration::from_millis(80));
            pb
        }
    }

    fn finish(
        &self,
        pb: &ProgressBar,
        stage: DeploymentStage,
        server_type: &str,
        location: &str,
        target: &str,
    ) {
        let msg = stage.success_message(server_type, location, target);
        let step = stage.number();
        if !self.tty {
            println!("  ✓ [{step}/{}] {msg}", self.total_steps);
            return;
        }
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "  {} [{step}/{}] {{msg}}",
                    "✓".green(),
                    self.total_steps
                ))
                .expect("invalid template"),
        );
        pb.finish_with_message(msg);
    }

    fn fail(
        &self,
        pb: &ProgressBar,
        stage: DeploymentStage,
        server_type: &str,
        location: &str,
        target: &str,
    ) {
        let msg = stage.failure_message(server_type, location, target);
        let step = stage.number();
        if !self.tty {
            println!("  ✗ [{step}/{}] {msg}", self.total_steps);
            return;
        }
        pb.set_style(
            ProgressStyle::default_spinner()
                .template(&format!(
                    "  {} [{step}/{}] {{msg}}",
                    "✗".red(),
                    self.total_steps
                ))
                .expect("invalid template"),
        );
        pb.finish_with_message(msg);
    }
}

struct DeploymentContext {
    config: Config,
    state: VpsState,
    server_type: String,
    location: String,
    image: String,
    daemon_binary: Option<std::path::PathBuf>,
}

impl DeploymentContext {
    fn load(no_wait: bool, server_type_override: Option<String>) -> Result<Self> {
        let config = Config::load()?;
        let state = VpsState::load()?;

        if state.is_provisioned() {
            bail!(
                "VPS already provisioned (server ID: {}, IP: {}). Run /down or `cloudcode down` first.",
                state.server_id.unwrap(),
                state.server_ip.as_deref().unwrap_or("unknown")
            );
        }

        let hetzner_config = config
            .hetzner
            .as_ref()
            .context("Hetzner not configured. Run /init or `cloudcode init` first.")?;

        // At least one AI provider (Claude or Codex) must be configured
        if config.claude.is_none() && config.codex.is_none() {
            anyhow::bail!(
                "No AI provider configured. At least one of Claude or Codex must be set up. Run /init or `cloudcode init` first."
            );
        }
        let _ = (hetzner_config, no_wait);

        let vps_config = config.vps.as_ref();
        let server_type = server_type_override
            .as_deref()
            .or_else(|| vps_config.and_then(|v| v.server_type.as_deref()))
            .unwrap_or("cx23")
            .to_string();
        let location = vps_config
            .and_then(|v| v.location.as_deref())
            .unwrap_or("nbg1")
            .to_string();
        let image = vps_config
            .and_then(|v| v.image.as_deref())
            .unwrap_or("ubuntu-24.04")
            .to_string();

        Ok(Self {
            config,
            state,
            server_type,
            location,
            image,
            daemon_binary: None,
        })
    }

    async fn run(mut self, no_wait: bool) -> Result<()> {
        let reporter = ConsoleReporter::new(TOTAL_STEPS);
        let ssh_pub_key_path = Config::ssh_pub_key_path()?;
        if !ssh_pub_key_path.exists() {
            let pb = reporter.start(
                DeploymentStage::GenerateCloudInit,
                &self.server_type,
                &self.location,
                self.target(),
            );
            reporter.fail(
                &pb,
                DeploymentStage::GenerateCloudInit,
                &self.server_type,
                &self.location,
                self.target(),
            );
            bail!(
                "SSH public key not found at {}. Run /init or `cloudcode init` first.",
                ssh_pub_key_path.display()
            );
        }

        self.confirm_risk_if_needed()?;
        println!("{}", "cloudcode up".bold().cyan());

        self.step_generate_cloud_init(&reporter).await?;
        self.step_create_hetzner_key(&reporter, &ssh_pub_key_path)
            .await?;
        self.step_provision_server(&reporter, &ssh_pub_key_path)
            .await?;

        if no_wait {
            println!(
                "\n{}",
                "VPS provisioned. Skipping cloud-init wait (--no-wait).".yellow()
            );
            println!(
                "{}",
                "Cloud-init is still running. Use /status or `cloudcode status` to check progress."
                    .yellow()
            );
            return Ok(());
        }

        self.step_wait_for_ssh(&reporter).await?;
        self.step_wait_for_cloud_init(&reporter).await?;
        self.step_verify_installation(&reporter).await?;
        self.step_prepare_daemon_binary(&reporter).await?;
        self.step_upload_daemon_binary(&reporter).await?;
        self.step_install_daemon_service(&reporter).await?;
        self.step_verify_daemon(&reporter).await?;

        self.print_success_summary();
        Ok(())
    }

    fn target(&self) -> &str {
        target_triple_for_server_type(&self.server_type)
    }

    fn confirm_risk_if_needed(&self) -> Result<()> {
        use std::io::IsTerminal;
        if std::io::stdout().is_terminal() {
            let cost_str = crate::hetzner::client::estimate_monthly_cost(&self.server_type)
                .map(|c| format!("~${:.2}/mo", c))
                .unwrap_or_else(|| "unknown cost".to_string());
            println!(
                "This will provision a {} server at {} on Hetzner. Continue? [Y/n]",
                self.server_type, cost_str
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("n") {
                println!("Aborted.");
                return Ok(());
            }

            println!(
                "Security model: the remote 'claude' user gets passwordless sudo, and Claude runs in bypass-permissions mode to support unattended remote control. Continue? [y/N]"
            );
            input.clear();
            std::io::stdin().read_line(&mut input)?;
            let trimmed = input.trim();
            if trimmed != "y" && trimmed != "Y" && !trimmed.eq_ignore_ascii_case("yes") {
                println!("Aborted.");
                return Ok(());
            }
        } else {
            println!(
                "{}",
                "Warning: cloudcode provisions a VPS where the 'claude' user has passwordless sudo and Claude runs in bypass-permissions mode."
                    .yellow()
            );
        }
        Ok(())
    }

    async fn step_generate_cloud_init(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let pb = reporter.start(
            DeploymentStage::GenerateCloudInit,
            &self.server_type,
            &self.location,
            self.target(),
        );
        let ssh_pub_key = fs::read_to_string(Config::ssh_pub_key_path()?)
            .context("Failed to read SSH public key")?
            .trim()
            .to_string();
        let cloud_init = provisioner::generate_cloud_init(
            &ssh_pub_key,
            self.config.claude.as_ref(),
        );
        let _ = &cloud_init;
        reporter.finish(
            &pb,
            DeploymentStage::GenerateCloudInit,
            &self.server_type,
            &self.location,
            self.target(),
        );
        Ok(())
    }

    async fn step_create_hetzner_key(
        &mut self,
        reporter: &ConsoleReporter,
        ssh_pub_key_path: &std::path::Path,
    ) -> Result<()> {
        let pb = reporter.start(
            DeploymentStage::CreateHetznerKey,
            &self.server_type,
            &self.location,
            self.target(),
        );
        let ssh_pub_key =
            fs::read_to_string(ssh_pub_key_path).context("Failed to read SSH public key")?;
        let client = HetznerClient::new(
            self.config
                .hetzner
                .as_ref()
                .context("Hetzner not configured")?
                .api_token
                .clone(),
        );
        let ssh_key_id = match client.create_ssh_key("cloudcode", ssh_pub_key.trim()).await {
            Ok(id) => {
                reporter.finish(
                    &pb,
                    DeploymentStage::CreateHetznerKey,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                id
            }
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::CreateHetznerKey,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                return Err(e.context("Failed to register SSH key with Hetzner"));
            }
        };
        self.state = VpsState {
            server_id: None,
            server_ip: None,
            ssh_key_id: Some(ssh_key_id),
            status: Some(VpsStatus::Creating),
        };
        self.state.save()?;
        Ok(())
    }

    async fn step_provision_server(
        &mut self,
        reporter: &ConsoleReporter,
        ssh_pub_key_path: &std::path::Path,
    ) -> Result<()> {
        let pb = reporter.start(
            DeploymentStage::ProvisionServer,
            &self.server_type,
            &self.location,
            self.target(),
        );
        let ssh_pub_key =
            fs::read_to_string(ssh_pub_key_path).context("Failed to read SSH public key")?;
        let client = HetznerClient::new(
            self.config
                .hetzner
                .as_ref()
                .context("Hetzner not configured")?
                .api_token
                .clone(),
        );
        let cloud_init = provisioner::generate_cloud_init(
            &ssh_pub_key,
            self.config.claude.as_ref(),
        );
        let (server_id, server_ip) = match client
            .create_server(
                "cloudcode",
                &self.server_type,
                &self.image,
                &self.location,
                vec![self.state.ssh_key_id.context("Missing SSH key id")?],
                &cloud_init,
            )
            .await
        {
            Ok(result) => {
                reporter.finish(
                    &pb,
                    DeploymentStage::ProvisionServer,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                result
            }
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::ProvisionServer,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                let error_text = e.to_string();
                if error_text.contains("resource_unavailable")
                    || error_text.contains("error during placement")
                {
                    println!(
                        "\n{}",
                        "Hetzner could not place that server type in the requested location right now."
                            .yellow()
                    );
                    println!(
                        "{}",
                        "Try `cloudcode up --server-type cax11`, retry the same command in a few minutes, or switch the default location/server type in your config."
                            .yellow()
                    );
                }
                return Err(e.context("Failed to create server"));
            }
        };

        self.state.server_id = Some(server_id);
        self.state.server_ip = Some(server_ip.clone());
        self.state.status = Some(VpsStatus::Initializing);
        self.state.save()?;
        Ok(())
    }

    async fn step_wait_for_ssh(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        // Clear any stale known_hosts entry for this IP so accept-new works
        if let Some(ref ip) = self.state.server_ip {
            if let Ok(known_hosts) = crate::ssh::known_hosts_path() {
                if known_hosts.exists() {
                    let _ = std::process::Command::new("ssh-keygen")
                        .args(["-R", ip, "-f", &known_hosts.to_string_lossy()])
                        .output();
                }
            }
        }

        let pb = reporter.start(
            DeploymentStage::WaitForSsh,
            &self.server_type,
            &self.location,
            self.target(),
        );
        match wait_for_ssh(&self.state, Duration::from_secs(120)).await {
            Ok(()) => reporter.finish(
                &pb,
                DeploymentStage::WaitForSsh,
                &self.server_type,
                &self.location,
                self.target(),
            ),
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::WaitForSsh,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                println!("\n{}: {}", "Warning".yellow().bold(), e);
                println!(
                    "{}",
                    "The server may still be starting. Try /status or `cloudcode status` later."
                        .yellow()
                );
                anyhow::bail!(
                    "SSH connectivity timed out. The VPS is provisioned — run /up again to retry from this point."
                );
            }
        }
        Ok(())
    }

    async fn step_wait_for_cloud_init(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let pb = reporter.start(
            DeploymentStage::WaitForCloudInit,
            &self.server_type,
            &self.location,
            self.target(),
        );
        match health::wait_for_cloud_init(&self.state, Duration::from_secs(600)).await? {
            CloudInitStatus::Ready => reporter.finish(
                &pb,
                DeploymentStage::WaitForCloudInit,
                &self.server_type,
                &self.location,
                self.target(),
            ),
            CloudInitStatus::Failed { error } => {
                reporter.fail(
                    &pb,
                    DeploymentStage::WaitForCloudInit,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                println!("\n{}: {}", "Error".red().bold(), error);
                println!(
                    "{}",
                    "Check logs with: /ssh -- cat /var/log/cloudcode-setup.log (or cloudcode ssh ...)"
                        .yellow()
                );
                self.state.status = Some(VpsStatus::Error);
                self.state.save()?;
                anyhow::bail!("Cloud-init failed: {}", error);
            }
        }
        Ok(())
    }

    async fn step_verify_installation(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let pb = reporter.start(
            DeploymentStage::VerifyInstallation,
            &self.server_type,
            &self.location,
            self.target(),
        );
        match health::verify_installation(&self.state).await {
            Ok(results) => {
                let all_ok = results.iter().all(|(_, ok)| *ok);
                if all_ok {
                    reporter.finish(
                        &pb,
                        DeploymentStage::VerifyInstallation,
                        &self.server_type,
                        &self.location,
                        self.target(),
                    );
                } else {
                    let missing: Vec<_> = results
                        .iter()
                        .filter(|(_, ok)| !ok)
                        .map(|(name, _)| name.as_str())
                        .collect();
                    reporter.fail(
                        &pb,
                        DeploymentStage::VerifyInstallation,
                        &self.server_type,
                        &self.location,
                        self.target(),
                    );
                    println!(
                        "\n{}: Some expected software is missing: {}",
                        "Warning".yellow().bold(),
                        missing.join(", ")
                    );
                }
            }
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::VerifyInstallation,
                    &self.server_type,
                    &self.location,
                    self.target(),
                );
                println!("\n{}: {}", "Warning".yellow().bold(), e);
            }
        }
        Ok(())
    }

    async fn step_prepare_daemon_binary(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let target = self.target().to_string();
        let pb = reporter.start(
            DeploymentStage::PrepareDaemonBinary,
            &self.server_type,
            &self.location,
            &target,
        );
        match get_daemon_binary(&target) {
            Ok(path) => {
                self.daemon_binary = Some(path);
                reporter.finish(
                    &pb,
                    DeploymentStage::PrepareDaemonBinary,
                    &self.server_type,
                    &self.location,
                    &target,
                );
            }
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::PrepareDaemonBinary,
                    &self.server_type,
                    &self.location,
                    &target,
                );
                println!("\n{}: {}", "Error".red().bold(), e);
                self.state.status = Some(VpsStatus::Error);
                self.state.save()?;
                anyhow::bail!("Failed to prepare daemon binary: {}", e);
            }
        }
        Ok(())
    }

    async fn step_upload_daemon_binary(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let target = self.target().to_string();
        let pb = reporter.start(
            DeploymentStage::UploadDaemonBinary,
            &self.server_type,
            &self.location,
            &target,
        );
        match upload_binary(
            &self.state,
            self.daemon_binary
                .as_ref()
                .context("Missing daemon binary")?,
        ) {
            Ok(()) => reporter.finish(
                &pb,
                DeploymentStage::UploadDaemonBinary,
                &self.server_type,
                &self.location,
                &target,
            ),
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::UploadDaemonBinary,
                    &self.server_type,
                    &self.location,
                    &target,
                );
                println!("\n{}: {}", "Error".red().bold(), e);
                self.state.status = Some(VpsStatus::Error);
                self.state.save()?;
                anyhow::bail!("Failed to upload daemon binary: {}", e);
            }
        }
        Ok(())
    }

    async fn step_install_daemon_service(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let target = self.target().to_string();
        let pb = reporter.start(
            DeploymentStage::InstallDaemonService,
            &self.server_type,
            &self.location,
            &target,
        );
        match install_daemon(&self.state, &self.config) {
            Ok(()) => reporter.finish(
                &pb,
                DeploymentStage::InstallDaemonService,
                &self.server_type,
                &self.location,
                &target,
            ),
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::InstallDaemonService,
                    &self.server_type,
                    &self.location,
                    &target,
                );
                println!("\n{}: {}", "Error".red().bold(), e);
                self.state.status = Some(VpsStatus::Error);
                self.state.save()?;
                anyhow::bail!("Failed to install daemon service: {}", e);
            }
        }
        Ok(())
    }

    async fn step_verify_daemon(&mut self, reporter: &ConsoleReporter) -> Result<()> {
        let target = self.target().to_string();
        let pb = reporter.start(
            DeploymentStage::VerifyDaemon,
            &self.server_type,
            &self.location,
            &target,
        );
        match verify_daemon(&self.state) {
            Ok(()) => {
                self.state.status = Some(VpsStatus::Running);
                self.state.save()?;
                reporter.finish(
                    &pb,
                    DeploymentStage::VerifyDaemon,
                    &self.server_type,
                    &self.location,
                    &target,
                );
            }
            Err(e) => {
                reporter.fail(
                    &pb,
                    DeploymentStage::VerifyDaemon,
                    &self.server_type,
                    &self.location,
                    &target,
                );
                println!("\n{}: {}", "Warning".yellow().bold(), e);
                self.state.status = Some(VpsStatus::Running);
                self.state.save()?;
            }
        }
        Ok(())
    }

    fn print_success_summary(&self) {
        println!(
            "\n  {} {}",
            "✓".green().bold(),
            "VPS provisioned and daemon deployed successfully!"
                .bold()
                .green(),
        );

        println!("\n  {}", "Next steps:".bold());
        println!(
            "    {}              # Create a session",
            "/spawn".cyan().bold()
        );
        println!(
            "    {}  # Connect interactively",
            "/open <name>".cyan().bold()
        );
        println!(
            "    {}",
            "(or use cloudcode spawn / cloudcode open <name> from CLI)".dimmed()
        );

        let claude_needs_oauth = self
            .config
            .claude
            .as_ref()
            .is_some_and(|c| matches!(c.auth_method, AuthMethod::Oauth));
        let codex_needs_oauth = self
            .config
            .codex
            .as_ref()
            .is_some_and(|c| matches!(c.auth_method, AuthMethod::Oauth));

        if claude_needs_oauth || codex_needs_oauth {
            println!(
                "\n  {}  {}",
                "!".yellow().bold(),
                "Login required".yellow().bold()
            );
            println!(
                "    Run {} after spawning to log in.",
                "/open <name>".cyan().bold()
            );
            if claude_needs_oauth {
                println!(
                    "    Claude will show a login URL — {} to copy it.",
                    "highlight and copy the URL manually".bold()
                );
                println!(
                    "    {}",
                    "(Pressing 'c' copies to the VPS clipboard, not your local machine.)".dimmed()
                );
            }
            if codex_needs_oauth {
                println!(
                    "    Codex will use device-code auth — {} when prompted.",
                    "select 'Device code'".bold()
                );
                println!(
                    "    {}",
                    "Visit the URL shown in your browser to authorize.".dimmed()
                );
            }
            if self.config.telegram.is_some() {
                println!(
                    "\n  {}  {}",
                    "!".yellow().bold(),
                    "Telegram will not work until login is complete.".yellow()
                );
            }
        }

        if self.config.telegram.is_some() {
            println!("\n  {}", "Telegram:".bold());
            println!("    Your bot is active! Message it to start chatting.");
            println!(
                "    Send {} to create a session, then type any message.",
                "/spawn".cyan().bold()
            );
        }
    }
}

pub async fn run(no_wait: bool, server_type_override: Option<String>) -> Result<()> {
    let plan = DeploymentPlan::for_mode(no_wait);
    let ctx = DeploymentContext::load(no_wait, server_type_override)?;
    let _ = plan.total_steps();
    let _ = plan.stages();
    ctx.run(no_wait).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_has_ten_steps_in_full_mode() {
        assert_eq!(DeploymentPlan::for_mode(false).stages().len(), 10);
    }

    #[test]
    fn plan_has_three_steps_in_no_wait_mode() {
        assert_eq!(DeploymentPlan::for_mode(true).stages().len(), 3);
    }

    #[test]
    fn stage_numbers_are_stable() {
        assert_eq!(DeploymentStage::GenerateCloudInit.number(), 1);
        assert_eq!(DeploymentStage::VerifyDaemon.number(), 10);
    }
}
