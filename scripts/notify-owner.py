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

The body may start with header lines (E-APPLIST.W3): `wu: <E-CODE.Wn>` names the work unit
it is about, `question: <name>` a question it asks. They are dropped from the message. Before
it sends, `--send` writes through its sibling scripts/router-app.sh the question line (set
question/<name>, when one is named) and one alert line (headline, one or two lines, and the work
unit and its keel://wu/<id> link when one is named), so a failed send never loses the desk
alert; the message then ends with that link. A mistyped header sends and writes nothing. A log
that cannot be written does not stop the send: one stderr line says why. `--dry-run` prints the
lines it would write and writes none.

No command-line argument can choose a file, credential, chat or endpoint. The
KEEL_NOTIFY_ENV, KEEL_NOTIFY_ACCESS, KEEL_NOTIFY_API_BASE and
KEEL_NOTIFY_TIMEOUT variables are the test seam: a normal invocation sets none.
"""

import argparse
import json
import os
import re
import signal
import socket
import subprocess
import sys
import unicodedata
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

# The update log (E-APPLIST.W3): router-app.sh beside this file, its grammars and caps.
APP = Path(__file__).resolve().parent / "router-app.sh"
APP_TIMEOUT = 10
WU = re.compile(r"E-[A-Z0-9]{1,16}\.W[0-9]{1,3}")
QUESTION = re.compile(r"[a-z0-9][a-z0-9-]{0,40}")
HEADER = re.compile(r"(wu|question):(.*)")
HEADLINE_MAX = 80
LINE_MAX = 200
# The alert's value stays under router-app.sh's 1,024-byte line (with its longest run id, n and
# date); the question's under its 512-byte value cap; both with room for jq's own escapes.
ALERT_BUDGET = 800
QUESTION_BUDGET = 480
NO_MORE = "See Telegram."
ELLIPSIS = "\u2026"


class NotifyError(Exception):
    pass


def one_line(value):
    return " ".join(str(value).splitlines()).strip()


def fail(message):
    print("notify-owner: " + one_line(message), file=sys.stderr)
    return 1


def plain(value):
    """One line of text: every control, format (a bidi override), line-separator or lone surrogate
    (a byte that is not UTF-8, as Python decodes an argument) a space, runs of space one."""
    shown = "".join(" " if unicodedata.category(c) in ("Cc", "Cf", "Cs", "Zl", "Zp") else c for c in value)
    return " ".join(shown.split())


def cut(value, most):
    return value if len(value) <= most else value[: most - 1].rstrip() + ELLIPSIS


def parse_body(text):
    """The body's header lines, its headline and next lines, and the text to send without the header."""
    kept = text.splitlines(keepends=True)
    found = {}
    at = 0
    while at < len(kept):
        match = HEADER.fullmatch(kept[at].rstrip("\r\n"))
        if not match:
            break
        name, value = match.group(1), match.group(2).strip()
        grammar = WU if name == "wu" else QUESTION
        if name in found:
            raise NotifyError(f"line {at + 1}: a second '{name}:' header; nothing was sent")
        if not grammar.fullmatch(value):
            want = "a work unit E-<CODE>.W<n>" if name == "wu" else "a short name of a-z, 0-9 and -"
            raise NotifyError(f"line {at + 1}: '{name}:' needs {want}; nothing was sent")
        found[name] = value
        at += 1
    rest = "".join(kept[at:])
    shown = [line for line in (plain(raw) for raw in rest.splitlines()) if line]
    if not shown:
        raise NotifyError("the message is empty")
    return {
        "wu": found.get("wu"),
        "question": found.get("question"),
        "headline": cut(shown[0], HEADLINE_MAX),
        "lines": [cut(line, LINE_MAX) for line in shown[1:3]] or [NO_MORE],
        "text": rest if at else text,
    }


def link(parsed):
    return f"keel://wu/{parsed['wu']}" if parsed["wu"] else None


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def fit(value, budget):
    """value, its second line dropped and then its longer text shortened, until its JSON fits budget bytes."""
    value = dict(value, lines=list(value["lines"]))
    while len(compact(value).encode("utf-8")) > budget:
        if len(value["lines"]) > 1:
            value["lines"].pop()
            continue
        headline, line = value["headline"], value["lines"][0]
        if max(len(headline), len(line)) <= 2:
            break
        if len(headline) >= len(line):
            value["headline"] = cut(headline, len(headline) * 9 // 10)
        else:
            value["lines"][0] = cut(line, len(line) * 9 // 10)
    return value


def alert_value(parsed):
    value = {"headline": parsed["headline"], "lines": parsed["lines"]}
    if parsed["wu"]:
        value.update(wu=parsed["wu"], link=link(parsed))
    return fit(value, ALERT_BUDGET)


def question_value(parsed):
    if not parsed["question"]:
        return None
    value = {"headline": parsed["headline"], "lines": parsed["lines"]}
    if parsed["wu"]:
        value["wu"] = parsed["wu"]
    return fit(value, QUESTION_BUDGET)


def message_text(parsed):
    """The text Telegram gets: the body without its header, then the link alone on the last line."""
    if not parsed["wu"]:
        return parsed["text"]
    return parsed["text"].rstrip("\r\n") + "\n" + link(parsed)


def writer_env():
    """The environment router-app.sh runs with: ours, minus the credential's names and the test seam."""
    return {key: value for key, value in os.environ.items() if not key.startswith(("KEEL_NOTIFY_", "TELEGRAM_")) and key != TOKEN_KEY}


def stop(child):
    """End a writer that outran its time, with every process it started: TERM to its group (its trap
    releases the lock), then KILL to whatever is left; the child is collected either way, and a pipe
    some stray process still holds never keeps the sender waiting."""
    for sig, grace in ((signal.SIGTERM, 1), (signal.SIGKILL, 5)):
        try:
            os.killpg(child.pid, sig)
        except (ProcessLookupError, PermissionError):
            pass
        try:
            child.communicate(timeout=grace)
            return
        except subprocess.TimeoutExpired:
            continue
    child.kill()
    try:
        child.wait(timeout=1)
    except subprocess.TimeoutExpired:
        pass


def write_line(args):
    """Run router-app.sh with args (no shell string), in a session of its own; its line number, or None and why."""
    try:
        child = subprocess.Popen(
            ["/bin/sh", str(APP), *args],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            encoding="utf-8",
            errors="replace",
            env=writer_env(),
            start_new_session=True,
        )
    except OSError as error:
        return None, f"cannot run /bin/sh: {error.strerror or error.__class__.__name__}"
    try:
        out, err = child.communicate(timeout=APP_TIMEOUT)
    except subprocess.TimeoutExpired:
        stop(child)
        return None, f"router-app.sh did not answer within {APP_TIMEOUT} s"
    except BaseException:
        stop(child)
        raise
    if child.returncode != 0:
        said = [line for line in err.splitlines() if line.strip()]
        why = one_line(said[-1]) if said else f"router-app.sh exited {child.returncode}"
        return None, why.removeprefix("router-app: ")
    match = re.search(r"\bn=([0-9]+)", out)
    return (match.group(1) if match else "?"), None


def write_lines(parsed):
    """The question line, then the alert line; the alert's number, or None. Each failure is one stderr line."""
    if not APP.is_file():
        print(f"notify-owner: no desk alert: {APP.name} is not beside the sender", file=sys.stderr)
        return None
    question = question_value(parsed)
    if question is not None:
        _, why = write_line(["set", f"question/{parsed['question']}", compact(question)])
        if why:
            print("notify-owner: no question line: " + one_line(why), file=sys.stderr)
    number, why = write_line(["alert", compact(alert_value(parsed))])
    if why:
        print("notify-owner: no desk alert: " + one_line(why), file=sys.stderr)
    return number


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
    parsed = parse_body(text)
    text = check_text(message_text(parsed))

    if args.dry_run is not None:
        # a byte that is not UTF-8 reaches the text as a lone surrogate: show it escaped, never fail on it
        sys.stdout.reconfigure(errors="backslashreplace")
        print(f"dry-run: valid sendMessage request ({len(text)} chars, {len(text.splitlines())} lines); no request sent")
        print(text, end="" if text.endswith("\n") else "\n")
        question = question_value(parsed)
        if question is not None:
            print(f"dry-run: would write set question/{parsed['question']} {compact(question)}")
        print(f"dry-run: would write alert {compact(alert_value(parsed))}")
        return 0

    number = write_lines(parsed)
    try:
        result = request("sendMessage", token, api_base, {"chat_id": chat, "text": text}, timeout)
    except NotifyError as error:
        if number is None:
            raise
        raise NotifyError(f"{error}; the desk alert was written (n {number})") from None
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
    if number is not None:
        print(f"desk alert n={number}")
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
