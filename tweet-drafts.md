# Cloudcode Launch Tweet Drafts

## Version A — Hype/Product Hunt energy

**Tweet 1**
> INTRODUCING CLOUDCODE
>
> A CLI tool that gives Claude Code and Codex their own persistent cloud server — and lets you control them from Telegram.
>
> $3.49/mo. Open source. Three commands to deploy.
>
> Your AI coding agent just got a phone number 🧵

**Image:** Split screen — left side is Telegram on a phone showing /spawn + AI response with code, right side is a terminal showing `cloudcode up` with the deployment steps completing.

**Tweet 2**
> brew install ssreeni1/tap/cloudcode
> cloudcode init
> cloudcode up
>
> The CLI provisions a Hetzner VPS, deploys a Rust daemon, and connects a Telegram bot — all automatically.
>
> From there, everything is managed through the CLI or your phone.

**Tweet 3**
> What you get:
>
> → Sessions that persist forever. Close your laptop, come back tomorrow, context is there
> → AI creates a file? Auto-sent to your Telegram
> → AI asking a question? Push notification on your phone
> → Switch Claude ↔ Codex with /provider
> → Peek at what the AI is doing in real time with /peek
> → Full interactive access with cloudcode open

**Tweet 4**
> I built this because I wanted to text Claude "build me an API" from the grocery store and come back to working code.
>
> Now I do that daily.
>
> github.com/ssreeni1/cloudcode
> MIT licensed. Star it, fork it, ship with it.

---

## Version B — Developer/technical angle

**Tweet 1**
> CLOUDCODE — a CLI that provisions and manages persistent Claude Code / Codex sessions on your own VPS
>
> Control from terminal or Telegram. Sessions never die. $3.49/mo Hetzner box.
>
> Open source. Ships today 🧵

**Image:** Terminal recording GIF showing: `cloudcode init` → `cloudcode up` → Telegram notification "Session created" → sending a coding task → getting a file back.

**Tweet 2**
> The CLI handles everything:
>
> cloudcode init — setup wizard (Hetzner, AI auth, Telegram)
> cloudcode up — provisions VPS + deploys daemon
> cloudcode spawn — creates a persistent session
> cloudcode open — interactive terminal access
> cloudcode status — VPS health, sessions, uptime
> cloudcode down — tears it all down
>
> Plus 12 Telegram commands for mobile control.

**Tweet 3**
> Under the hood:
>
> • Rust daemon managing tmux sessions
> • Claude Code or OpenAI Codex as the AI backend
> • Telegram bot with spawn, kill, peek, reply, context, provider...
> • Hetzner VPS provisioned automatically via cloud-init
> • SSH tunnel with auto-reconnect for CLI access
> • Auto-detects when AI needs input, notifies your phone

**Tweet 4**
> The best part: it's YOUR server. YOUR data. $3.49/mo.
>
> No SaaS middleman. No usage limits beyond your API plan. Kill it when you're done, spin it back up when you need it.
>
> brew install ssreeni1/tap/cloudcode
> github.com/ssreeni1/cloudcode

---

## Version C — Story/personal angle

**Tweet 1**
> I wanted to text an AI "build this feature" from my phone and come back to working code.
>
> So I built cloudcode — a CLI that deploys persistent Claude/Codex sessions to a cloud server and lets you manage them from Telegram.
>
> Open source. Shipping today 🧵

**Image:** Photo of a phone with Telegram open, showing a conversation with the cloudcode bot — a /spawn, a coding request, and a file attachment coming back. Casual, authentic, not a mockup.

**Tweet 2**
> The problem:
>
> Claude Code sessions die when you close the tab. Codex needs a terminal. Neither works from your phone. Context is constantly lost.
>
> The fix:
>
> A CLI that provisions a $3.49/mo VPS, deploys a Rust daemon, and gives you persistent sessions you can control from anywhere — terminal or Telegram.

**Tweet 3**
> brew install ssreeni1/tap/cloudcode
> cloudcode init
> cloudcode up
>
> Then open Telegram:
>
> /spawn my-project
> "Build me a REST API with auth"
>
> Go make coffee. Come back to code + files in your chat.
>
> Or attach interactively: cloudcode open my-project

**Tweet 4**
> Things that hit different:
>
> → cloudcode status from any terminal shows your VPS, sessions, uptime
> → /peek shows exactly what the AI is doing right now
> → /waiting tells you which sessions need your input
> → Files the AI creates auto-send to Telegram
> → Switch Claude ↔ Codex per session
> → Sessions survive reboots, disconnects, everything

**Tweet 5**
> 100% open source. MIT license. Built in Rust.
>
> github.com/ssreeni1/cloudcode
>
> Star it if you think AI coding agents should be persistent, portable, and controlled from your pocket.

---

## Version D — Short and punchy (3 tweets only)

**Tweet 1**
> CLOUDCODE
>
> A CLI that deploys Claude Code / Codex to a persistent cloud server and lets you control it from Telegram.
>
> Sessions never die. Files auto-sent. $3.49/mo. Open source.
>
> 🧵

**Image:** Clean screenshot of Telegram conversation — user sends "build a game of life in python", Claude responds with description, then a .py file appears as an attachment below.

**Tweet 2**
> brew install ssreeni1/tap/cloudcode
> cloudcode init    # setup wizard
> cloudcode up      # provisions VPS + deploys everything
> cloudcode spawn   # create a session
> cloudcode open    # attach interactively
>
> Or just message your Telegram bot. Full mobile control. Push notifications when the AI needs you.

**Tweet 3**
> Your AI agent should run 24/7, not die when you close a tab.
>
> github.com/ssreeni1/cloudcode
>
> MIT licensed. PRs welcome. Star if you agree.

---

## Version E — Maximum viral, slightly unhinged

**Tweet 1**
> I gave Claude Code a VPS and a phone number
>
> cloudcode is a CLI that deploys persistent AI coding sessions to a $3.49/mo server and lets you control them from Telegram
>
> I literally send coding tasks from the grocery store now
>
> open source 🧵

**Image:** Actual screenshot from your phone of a Telegram conversation where you asked Claude to build something real.

**Tweet 2**
> the workflow:
>
> 1. brew install ssreeni1/tap/cloudcode
> 2. cloudcode init → cloudcode up
> 3. message your Telegram bot: /spawn
> 4. "build me X"
> 5. go live your life
> 6. come back to finished code + files in your chat
>
> sessions never die. switch between Claude and Codex. peek at progress anytime.

**Tweet 3**
> things I didn't expect:
>
> → the AI finishes a task and you get a notification while walking your dog
> → /peek lets you watch it think in real time
> → it detects when Claude is asking "do you want to proceed?" and pings you
> → files auto-send. python scripts, screenshots, whatever it creates
>
> it just works

**Tweet 4**
> your own server. your own data. no SaaS wrapper. no middleman.
>
> just a Rust daemon, a Telegram bot, and tmux sessions that don't quit.
>
> github.com/ssreeni1/cloudcode
>
> MIT licensed. built in public. star it or fork it idc just ship things

---

## Image suggestions ranked

1. **Best:** Real Telegram screenshot on your phone showing /spawn → coding request → file attachment received. Authentic > polished.
2. **Good:** Terminal showing `cloudcode up` completing all 10 steps, then a Telegram notification appearing
3. **Good:** Split screen — terminal on left, Telegram on right, same session
4. **Okay:** Terminal GIF of the full flow (init → up → spawn → open)
5. **Skip:** Architecture diagrams, generic AI imagery
