"""Entry point for the frozen build.

A file of its own rather than pointing PyInstaller at the console script: the generated script lives
inside the venv, so freezing it would bake this machine's paths into the bundle.
"""

from __future__ import annotations

import multiprocessing
import sys

from pai_rag_service.cli import main

if __name__ == "__main__":
    # Without this a frozen child process re-runs the whole program instead of the worker function,
    # which on macOS shows up as the app starting itself over and over.
    multiprocessing.freeze_support()
    sys.exit(main())
