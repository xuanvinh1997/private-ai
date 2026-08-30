"""The PySide6 desktop application.

Nothing outside this package may import Qt: the ingestion worker and the MCP servers load
``private_ai.core`` in processes where PySide6 is not installed at all.
"""

from __future__ import annotations
