#!/usr/bin/env python3
"""Opt-in Unix/tmux startup and composer baseline; never submits a model request."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import statistics
import subprocess
import tempfile
import time
import uuid


def summarize(samples):
    ordered = sorted(samples)
    return {
        "samples_ms": samples,
        "median_ms": statistics.median(ordered),
        "p95_ms": ordered[math.ceil(len(ordered) * 0.95) - 1],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--startup-samples", type=int, default=5)
    parser.add_argument("--input-samples", type=int, default=20)
    args = parser.parse_args()
    if not 1 <= args.startup_samples <= 30 or not 1 <= args.input_samples <= 100:
        parser.error("startup samples must be 1..30 and input samples 1..100")
    if not shutil.which("tmux"):
        parser.error("this opt-in real-terminal probe requires tmux")
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    report = {
        "binary_sha256": digest,
        "method": "tmux launch to first composer; one ASCII key after a Unicode draft to capture-pane observation",
        "poll_interval_ms": 5,
        "limitations": "includes tmux launch/capture overhead; warm OS cache; no model turn; local only",
        "sizes": {},
    }
    for width, height in [(80, 24), (120, 40), (200, 50)]:
        startup, inputs = [], []
        for sample in range(args.startup_samples):
            with tempfile.TemporaryDirectory(
                prefix="codex-terminal-baseline-"
            ) as directory:
                root = Path(directory)
                home, workspace = root / "home", root / "workspace"
                home.mkdir()
                workspace.mkdir()
                (home / "config.toml").write_text(
                    'model = "gpt-5.6-terra"\n'
                    'model_provider = "openai"\n'
                    "suppress_unstable_features_warning = true\n"
                    f'[projects.{json.dumps(str(workspace))}]\ntrust_level = "trusted"\n',
                    encoding="utf-8",
                )
                (home / "auth.json").write_text(
                    '{"OPENAI_API_KEY":"dummy","tokens":null,"last_refresh":null}',
                    encoding="utf-8",
                )
                env = {
                    "PATH": os.environ["PATH"],
                    "HOME": str(home),
                    "CODEX_HOME": str(home),
                    "TERM": "xterm-256color",
                    "LANG": os.environ.get("LANG", "C.UTF-8"),
                    "OTEL_SDK_DISABLED": "true",
                }
                socket = f"codex-baseline-{uuid.uuid4().hex}"

                def tmux(*command, check=True):
                    return subprocess.run(
                        ["tmux", "-u", "-L", socket, *command],
                        env=env,
                        capture_output=True,
                        text=True,
                        encoding="utf-8",
                        timeout=5,
                        check=check,
                    ).stdout

                def wait_for(text, present=True):
                    deadline = time.monotonic() + 15
                    while time.monotonic() < deadline:
                        screen = tmux("capture-pane", "-p", "-t", "baseline:0.0")
                        if (text in screen) == present:
                            return screen
                        time.sleep(0.005)
                    raise TimeoutError(
                        f"terminal did not reach expected state: {text!r}"
                    )

                command = [
                    str(binary),
                    "--no-alt-screen",
                    "-C",
                    str(workspace),
                    "-c",
                    "analytics.enabled=false",
                    "-c",
                    "check_for_update_on_startup=false",
                    "-c",
                    'openai_base_url="http://127.0.0.1:9/v1"',
                ]
                try:
                    start = time.perf_counter_ns()
                    tmux(
                        "new-session",
                        "-d",
                        "-x",
                        str(width),
                        "-y",
                        str(height),
                        "-s",
                        "baseline",
                        "--",
                        shlex.join(command),
                    )
                    wait_for("Ask Codex to do anything")
                    startup.append((time.perf_counter_ns() - start) / 1_000_000)
                    if sample == args.startup_samples - 1:
                        for index in range(args.input_samples):
                            prefix = f"scan-{index:03} 日本語 cafe\u0301 "
                            tmux("send-keys", "-t", "baseline:0.0", "-l", prefix)
                            wait_for(prefix.rstrip())
                            token = prefix + "x"
                            start = time.perf_counter_ns()
                            tmux("send-keys", "-t", "baseline:0.0", "-l", "x")
                            screen = wait_for(token)
                            inputs.append((time.perf_counter_ns() - start) / 1_000_000)
                            (args.output / f"composer-{width}x{height}.txt").write_text(
                                screen, encoding="utf-8"
                            )
                            tmux("send-keys", "-t", "baseline:0.0", "C-u")
                            wait_for(token, present=False)
                        token = "resize 日本語 cafe\u0301"
                        tmux("send-keys", "-t", "baseline:0.0", "-l", token)
                        for resize_width, resize_height in [
                            (80, 24),
                            (120, 40),
                            (200, 50),
                        ]:
                            tmux(
                                "resize-window",
                                "-t",
                                "baseline:0",
                                "-x",
                                str(resize_width),
                                "-y",
                                str(resize_height),
                            )
                            suffix = f" [{resize_width}]"
                            token += suffix
                            tmux("send-keys", "-t", "baseline:0.0", "-l", suffix)
                            wait_for(token)
                finally:
                    tmux("kill-server", check=False)
        report["sizes"][f"{width}x{height}"] = {
            "first_composer": summarize(startup),
            "input_to_observed_frame": summarize(inputs),
            "resize_preserves_unicode_draft": True,
        }
    (args.output / "terminal-metrics.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
