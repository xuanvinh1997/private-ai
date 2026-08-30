"""``python -m private_ai.worker`` — the same entry point as the console script."""

from __future__ import annotations

from private_ai.worker.loop import run

if __name__ == "__main__":
    run()
