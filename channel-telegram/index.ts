#!/usr/bin/env node

// Channel-Telegram: minimal Telegram inbound transport.
// Polls Telegram for messages and forwards them to the Rust daemon's HTTP dispatch endpoint.

const log = (...args: unknown[]) => console.error("[channel-telegram]", ...args);

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

const BOT_TOKEN = process.env.CLOUDCODE_TG_BOT_TOKEN;
const OWNER_ID = process.env.CLOUDCODE_TG_OWNER_ID;
const DISPATCH_URL =
  process.env.CLOUDCODE_DISPATCH_URL ?? "http://localhost:8789/dispatch";

if (!BOT_TOKEN) {
  log("FATAL: CLOUDCODE_TG_BOT_TOKEN is not set");
  process.exit(1);
}
if (!OWNER_ID) {
  log("FATAL: CLOUDCODE_TG_OWNER_ID is not set");
  process.exit(1);
}

const ownerIdNum = Number(OWNER_ID);
if (Number.isNaN(ownerIdNum)) {
  log("FATAL: CLOUDCODE_TG_OWNER_ID is not a valid number");
  process.exit(1);
}

const TG_API = `https://api.telegram.org/bot${BOT_TOKEN}`;

// ---------------------------------------------------------------------------
// Telegram long-polling
// ---------------------------------------------------------------------------

interface TgUpdate {
  update_id: number;
  message?: {
    chat: { id: number; type: string };
    from?: { id: number };
    text?: string;
  };
}

let offset = 0;
let backoff = 1; // seconds, for exponential backoff on errors
const MAX_BACKOFF = 30;

async function getUpdates(): Promise<TgUpdate[]> {
  const url = `${TG_API}/getUpdates?timeout=30&offset=${offset}`;
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`Telegram getUpdates failed: ${res.status} ${res.statusText}`);
  }
  const body = (await res.json()) as { ok: boolean; result: TgUpdate[] };
  if (!body.ok) {
    throw new Error("Telegram getUpdates returned ok=false");
  }
  return body.result;
}

// ---------------------------------------------------------------------------
// Dispatch to Rust daemon
// ---------------------------------------------------------------------------

async function dispatch(chatId: number, text: string): Promise<void> {
  const res = await fetch(DISPATCH_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ chat_id: chatId, text }),
  });
  if (!res.ok) {
    log(`dispatch error: ${res.status} ${res.statusText}`);
  }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

async function poll(): Promise<void> {
  log(`starting — owner=${ownerIdNum} dispatch=${DISPATCH_URL}`);

  while (true) {
    try {
      const updates = await getUpdates();
      backoff = 1; // reset on success

      for (const u of updates) {
        offset = u.update_id + 1;

        const msg = u.message;
        if (!msg) continue;

        // Gate: private chat from the owner only
        if (msg.chat.type !== "private") continue;
        if (msg.from?.id !== ownerIdNum) continue;
        if (!msg.text) continue;

        log(`msg from ${msg.from.id}: ${msg.text.slice(0, 80)}`);
        await dispatch(msg.chat.id, msg.text);
      }
    } catch (err) {
      log(`poll error (backoff ${backoff}s):`, err instanceof Error ? err.message : err);
      await sleep(backoff * 1000);
      backoff = Math.min(backoff * 2, MAX_BACKOFF);
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

poll();
