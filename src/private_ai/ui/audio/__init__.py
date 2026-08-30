"""Microphone capture for the composer's dictation button."""

from __future__ import annotations

from private_ai.ui.audio.capture import (
    FRAME_SAMPLES,
    STATE_IDLE,
    STATE_RECORDING,
    STATE_TRANSCRIBING,
    TARGET_SAMPLE_RATE,
    MicrophoneCapture,
)

__all__ = [
    "FRAME_SAMPLES",
    "STATE_IDLE",
    "STATE_RECORDING",
    "STATE_TRANSCRIBING",
    "TARGET_SAMPLE_RATE",
    "MicrophoneCapture",
]
