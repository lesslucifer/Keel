#!/usr/bin/env python3
"""Send the router's owner message to the paired Telegram bot.

The router runs this in the same turn it stages an owner action. The bot token
and the one chat it may write to are read from the Claude Code Telegram
channel's own state; neither is ever printed. `--check` asks Telegram who the
bot is, `--dry-run` validates and prints without sending, `--send` posts and
prints the Telegram message id.

The message body is the text the router already writes for the push channel:
short, commands on their own lines, `cd`-prefixed.
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_ENV = Path.home() / ".claude" / "channels" / "telegram" / ".env"
DEFAULT_ACCESS = Path.home() / ".claude" / "channels" / "telegram" / "access.json"
DEFAULT_BODY = Path(".router") / "notify" / "owner.md"
DEFAULT_API_BASE = "https://api.telegram.org"
TOKEN_KEY = "TELEGRAM_BOT_TOKEN"
MAX_TEXT = 4096
USE_FILE = object()


class NotifyError(Exception):
    pass


def one_line(value):
    return " ".join(str(value).splitlines()).strip()


def fail(message):
    print("notify-owner: " + one_line(message), file=sys.stderr)
    return 1


def read_env(path):
    try:
        lines = Path(path).read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise NotifyError(f"cannot read {path}: {error.strerror or error}") from None
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        if separator and key.strip() == TOKEN_KEY:
            value = value.strip().strip('"').strip("'")
            if value:
                return value
    raise NotifyError(f"no {TOKEN_KEY} in {path}")


def read_chat(path):
    try:
        document = json.loads(Path(path).read_text(encoding="utf-8"))
    except OSError as error:
        raise NotifyError(f"cannot read {path}: {error.strerror or error}") from None
    except ValueError:
        raise NotifyError(f"{path} is not JSON") from None
    allowed = document.get("allowFrom") if isinstance(document, dict) else None
    if not isinstance(allowed, list) or not allowed:
        raise NotifyError(f"no paired chat in {path}")
    chat = allowed[0]
    if isinstance(chat, bool) or not isinstance(chat, (str, int)):
        raise NotifyError(f"no paired chat in {path}")
    chat = str(chat).strip()
    if not chat or not chat.lstrip("-").isdigit():
        raise NotifyError(f"no paired chat in {path}")
    return chat


def read_body(path):
    try:
        body = Path(path).read_text(encoding="utf-8")
    except OSError as error:
        raise NotifyError(f"cannot read {path}: {error.strerror or error}; write the message there first") from None
    return body


def check_text(text):
    if not text or not text.strip():
        raise NotifyError("the message is empty")
    if len(text) > MAX_TEXT:
        raise NotifyError(f"the message is {len(text)} characters; Telegram accepts {MAX_TEXT}")
    return text


def request(method, token, api_base, payload):
    url = api_base.rstrip("/") + "/bot" + token + "/" + method
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    headers = {} if data is None else {"Content-Type": "application/json"}
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=headers), timeout=15) as response:
            raw = response.read()
    except urllib.error.HTTPError as error:
        detail = ""
        try:
            body = json.loads(error.read().decode("utf-8", "replace"))
            detail = one_line(body.get("description", "")) if isinstance(body, dict) else ""
        except Exception:
            detail = ""
        suffix = f" ({detail})" if detail else ""
        raise NotifyError(f"Telegram returned HTTP {error.code}{suffix}") from None
    except urllib.error.URLError as error:
        raise NotifyError(f"cannot reach the Telegram API: {one_line(error.reason)}") from None
    except Exception:
        raise NotifyError("the Telegram request failed") from None
    try:
        answer = json.loads(raw.decode("utf-8", "replace"))
    except ValueError:
        raise NotifyError("Telegram returned a response that is not JSON") from None
    if not isinstance(answer, dict) or answer.get("ok") is not True:
        detail = one_line(answer.get("description", "")) if isinstance(answer, dict) else ""
        raise NotifyError("Telegram refused the request" + (f": {detail}" if detail else ""))
    return answer.get("result")


def main(argv):
    parser = argparse.ArgumentParser(prog="notify-owner.py", description="Send the router's owner message to the paired Telegram bot.")
    parser.add_argument("--send", nargs="?", const=USE_FILE, metavar="TEXT")
    parser.add_argument("--dry-run", nargs="?", const=USE_FILE, metavar="TEXT")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--body", metavar="PATH", help=f"message file for a bare --send/--dry-run (default {DEFAULT_BODY})")
    parser.add_argument("--api-base", default=os.environ.get("KEEL_NOTIFY_API_BASE", DEFAULT_API_BASE))
    parser.add_argument("--env-file", default=os.environ.get("KEEL_NOTIFY_ENV", str(DEFAULT_ENV)))
    parser.add_argument("--access-file", default=os.environ.get("KEEL_NOTIFY_ACCESS", str(DEFAULT_ACCESS)))
    args = parser.parse_args(argv)

    modes = int(args.send is not None) + int(args.dry_run is not None) + int(args.check)
    if modes != 1:
        parser.error("give exactly one of --send, --dry-run, --check")
    if args.check:
        if args.body is not None:
            parser.error("--body belongs to --send/--dry-run")

    api_base = args.api_base.strip()
    if not api_base.startswith("https://") and not api_base.startswith("http://"):
        raise NotifyError(f"--api-base is not an http(s) URL: {api_base}")

    token = read_env(args.env_file)

    if args.check:
        bot = request("getMe", token, api_base, None)
        if not isinstance(bot, dict) or not bot.get("id"):
            raise NotifyError("Telegram did not name the bot")
        username = bot.get("username")
        who = f"@{username}" if isinstance(username, str) and username else "bot"
        print(f"check ok: {who} (id {bot['id']})")
        return 0

    text = args.send if args.send is not None else args.dry_run
    if text is USE_FILE:
        text = read_body(args.body if args.body is not None else DEFAULT_BODY)
    elif args.body is not None:
        parser.error("give text or --body, not both")
    text = check_text(text)
    chat = read_chat(args.access_file)

    if args.dry_run is not None:
        print(f"dry-run: would sendMessage to chat {chat} ({len(text)} chars, {len(text.splitlines())} lines)")
        print(text, end="" if text.endswith("\n") else "\n")
        return 0

    result = request("sendMessage", token, api_base, {"chat_id": chat, "text": text})
    message_id = result.get("message_id") if isinstance(result, dict) else None
    if message_id is None:
        raise NotifyError("Telegram did not return a message id")
    print(f"id {message_id}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except NotifyError as error:
        sys.exit(fail(str(error)))
    except SystemExit:
        raise
    except KeyboardInterrupt:
        sys.exit(1)
    except Exception:
        sys.exit(fail("the request failed"))
