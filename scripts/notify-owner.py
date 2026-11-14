#!/usr/bin/env python3
"""Send the router's owner message to the paired Telegram bot.

The router runs this in the same turn it stages an owner action. The bot token
and the one chat it may write to are read from the Claude Code Telegram
channel's own state; neither the token nor the chat id is ever printed.
`--check` asks Telegram who the bot is and sends nothing; `--dry-run` validates
the body and prints the request without reaching the network; `--send` posts and
prints the Telegram message id.

A body read from `.router/notify/owner.md` must be writable. The message is
reported with `id N` only after that file is emptied, so a later bare `--send`
cannot resend it. A body that is not writable fails before sending (nothing is
sent); a clear that fails after the send names the sent id and the file, and
prints no `id N`.

No command-line argument can choose a file, credential, chat or endpoint. The
KEEL_NOTIFY_ENV, KEEL_NOTIFY_ACCESS, KEEL_NOTIFY_API_BASE and
KEEL_NOTIFY_TIMEOUT variables are the test seam: a normal invocation sets none.
"""

import argparse
import json
import os
import socket
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

DEFAULT_ENV = Path.home() / ".claude" / "channels" / "telegram" / ".env"
DEFAULT_ACCESS = Path.home() / ".claude" / "channels" / "telegram" / "access.json"
DEFAULT_BODY = Path(".router") / "notify" / "owner.md"
DEFAULT_API_BASE = "https://api.telegram.org"
DEFAULT_TIMEOUT = 15.0
TOKEN_KEY = "TELEGRAM_BOT_TOKEN"
MAX_TEXT = 4096
LOOPBACK = {"127.0.0.1", "localhost", "::1"}
USE_FILE = object()


class NotifyError(Exception):
    pass


def one_line(value):
    return " ".join(str(value).splitlines()).strip()


def fail(message):
    print("notify-owner: " + one_line(message), file=sys.stderr)
    return 1


def check_api_base(value):
    value = value.strip()
    parts = urllib.parse.urlsplit(value)
    host = (parts.hostname or "").lower()
    if not parts.netloc:
        raise NotifyError("KEEL_NOTIFY_API_BASE has no host")
    if parts.scheme != "https" and not (parts.scheme == "http" and host in LOOPBACK):
        raise NotifyError("KEEL_NOTIFY_API_BASE must be https (http is allowed for loopback only)")
    return value


def check_timeout(value):
    try:
        seconds = float(value)
    except ValueError:
        raise NotifyError("KEEL_NOTIFY_TIMEOUT is not a number") from None
    if not 0 < seconds <= 120:
        raise NotifyError("KEEL_NOTIFY_TIMEOUT must be between 0 and 120 seconds")
    return seconds


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


def request(method, token, api_base, payload, timeout):
    url = api_base.rstrip("/") + "/bot" + token + "/" + method
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    headers = {} if data is None else {"Content-Type": "application/json"}
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=headers), timeout=timeout) as response:
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
    except (TimeoutError, socket.timeout):
        raise NotifyError("cannot reach the Telegram API: timed out") from None
    except urllib.error.URLError as error:
        # Only an OS strerror or the exception class is safe: a raw reason can echo the
        # request target, token included.
        reason = getattr(error, "reason", None)
        detail = getattr(reason, "strerror", None)
        if not detail and isinstance(reason, BaseException):
            detail = reason.__class__.__name__
        suffix = f": {one_line(detail)}" if isinstance(detail, str) and detail else ""
        raise NotifyError("cannot reach the Telegram API" + suffix) from None
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
    args = parser.parse_args(argv)

    if int(args.send is not None) + int(args.dry_run is not None) + int(args.check) != 1:
        parser.error("give exactly one of --send, --dry-run, --check")

    api_base = check_api_base(os.environ.get("KEEL_NOTIFY_API_BASE", DEFAULT_API_BASE))
    timeout = check_timeout(os.environ.get("KEEL_NOTIFY_TIMEOUT", str(DEFAULT_TIMEOUT)))
    token = read_env(os.environ.get("KEEL_NOTIFY_ENV", str(DEFAULT_ENV)))

    if args.check:
        bot = request("getMe", token, api_base, None, timeout)
        if not isinstance(bot, dict) or not bot.get("id"):
            raise NotifyError("Telegram did not name the bot")
        username = bot.get("username")
        who = f"@{username}" if isinstance(username, str) and username else "bot"
        print(f"check ok: {who} (id {bot['id']})")
        return 0

    text = args.send if args.send is not None else args.dry_run
    from_file = text is USE_FILE
    if from_file:
        text = read_body(DEFAULT_BODY)
    text = check_text(text)
    if from_file and not os.access(DEFAULT_BODY, os.W_OK):
        raise NotifyError(f"cannot clear {DEFAULT_BODY}: it is not writable; the message was not sent")
    chat = read_chat(os.environ.get("KEEL_NOTIFY_ACCESS", str(DEFAULT_ACCESS)))

    if args.dry_run is not None:
        print(f"dry-run: valid sendMessage request ({len(text)} chars, {len(text.splitlines())} lines); no request sent")
        print(text, end="" if text.endswith("\n") else "\n")
        return 0

    result = request("sendMessage", token, api_base, {"chat_id": chat, "text": text}, timeout)
    message_id = result.get("message_id") if isinstance(result, dict) else None
    if message_id is None:
        raise NotifyError("Telegram did not return a message id")
    if from_file:
        try:
            DEFAULT_BODY.write_text("", encoding="utf-8")
        except OSError as error:
            raise NotifyError(
                f"the message was sent (id {message_id}), but {DEFAULT_BODY} could not be cleared: {error.strerror or error}; a later bare --send would resend it"
            ) from None
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
