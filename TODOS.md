# TODOS

## File detection for /reply and /type paths
**Priority:** Medium
**Why:** When users answer interactive prompts via Telegram `/reply` or `/type`, the daemon calls `send_keys()` but never snapshots files before/after. Files generated in response (screenshots, code) are silently missed. Users expect parity with `/send`.
**Approach:** Reuse the question poller's output stabilization logic. After `send_keys()`, wait for pane output to stabilize, then run `snapshot_files()` diff and upload new files to Telegram.
**Depends on:** Question poller stabilization heuristic (already implemented in `question_poller.rs`).
**Files:** `crates/cloudcode-daemon/src/telegram/handlers.rs`, `crates/cloudcode-daemon/src/session/manager.rs`

## Telegram user-scoped access control
**Priority:** Medium
**Why:** `owner_id` is matched against `msg.chat.id`, not the sender's user ID. If a group chat ID is configured, all participants can control the VPS. Users may not realize this distinction.
**Approach:** Check `msg.from().map(|u| u.id)` against the configured owner_id in addition to the chat_id check. Both must match for commands to execute.
**Depends on:** Nothing.
**Files:** `crates/cloudcode-daemon/src/telegram/handlers.rs`, `crates/cloudcode-daemon/src/telegram/bot.rs`

## AWS EC2 provider implementation
**Priority:** P2
**Why:** AWS is the largest cloud provider by market share. Adding EC2 support covers the biggest user segment that can't use cloudcode today because they don't have Hetzner/DO/Fly accounts. Competitive necessity — agentcomputer.ai likely supports AWS-hosted compute.
**Approach:** Implement `CloudProvider` trait for EC2. Requires: VPC creation (or default VPC detection), security group setup (SSH-only, matching current UFW model), key pair management via EC2 API, and region selection. Use `aws-sdk-ec2` crate. Cloud-init works natively on EC2.
**Effort:** L human / M with CC
**Depends on:** CloudProvider trait (multi-provider PR must ship first).
**Files:** New `crates/cloudcode-cli/src/aws/` module, `config.rs` (add AwsConfig), `commands/init.rs` (add AWS flow)

## GCP Compute Engine + Azure VM provider implementations
**Priority:** P2
**Why:** Completes the "big three" cloud coverage. Many enterprise and startup devs have GCP or Azure credits/accounts. Together with AWS, covers ~90% of cloud users.
**Approach:** Implement `CloudProvider` trait for each. GCP: use `google-cloud-compute` crate, handle project/zone selection. Azure: use `azure_mgmt_compute` crate, handle resource group/subscription. Both support cloud-init.
**Effort:** L each human / M each with CC
**Depends on:** CloudProvider trait, ideally ships after AWS to validate the trait handles complex providers.
**Files:** New `crates/cloudcode-cli/src/gcp/` and `crates/cloudcode-cli/src/azure/` modules

## Daytona dev environment integration
**Priority:** P3
**Why:** Daytona is a dev environment management platform that provisions workspaces on various backends. It's a different integration model — instead of cloudcode managing raw VMs, it delegates to Daytona's orchestration. This could make cloudcode work on any backend Daytona supports (Docker, Kubernetes, cloud VMs) without per-provider implementations.
**Approach:** Implement `CloudProvider` trait as a "meta-provider" that calls Daytona's API to create/manage workspaces. Daytona handles the underlying infrastructure. Requires understanding Daytona's workspace API and how SSH access works through it.
**Effort:** M human / S with CC
**Depends on:** CloudProvider trait. Needs design thinking on how Daytona's workspace model maps to cloudcode's VPS model.
**Files:** New `crates/cloudcode-cli/src/daytona/` module
