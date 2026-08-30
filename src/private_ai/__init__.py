"""Private AI — a local-first, multi-purpose AI desktop application.

One base source tree serves every surface: the PySide6 desktop UI, the ingestion
worker, and the MCP servers all import from here and share a single service
container. There is no HTTP hop between the UI and the domain layer.
"""

__version__ = "0.2.0"
