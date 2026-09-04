# PyInstaller spec for the RAG sidecar.
#
# onedir, not onefile: a onefile build unpacks itself into a temp directory on every launch, and this
# process carries ~300 MB of ONNX runtime and tokenizers. The app starts it lazily on the first RAG
# call, and a user waiting for a document search should not pay an unpack.
#
# The hidden-import list exists because three of this service's dependencies resolve modules by
# string at runtime, which no static analysis follows: markitdown picks a converter per file type,
# onnxruntime picks an execution provider, and qdrant-client picks REST or gRPC.

from PyInstaller.utils.hooks import collect_all, collect_submodules

datas, binaries, hiddenimports = [], [], []

for package in (
    "markitdown",
    "onnxruntime",
    "tokenizers",
    "surrealdb",
    "qdrant_client",
    "pypdfium2",
    "huggingface_hub",
):
    found_datas, found_binaries, found_hidden = collect_all(package)
    datas += found_datas
    binaries += found_binaries
    hiddenimports += found_hidden

# `mcp` cannot go through collect_all: importing `mcp.cli` calls `sys.exit(1)` when its optional
# `typer` dependency is absent, which kills the analysis rather than skipping a module. The service
# speaks MCP over stdio and never touches that command line.
hiddenimports += collect_submodules(
    "mcp", filter=lambda name: not name.startswith("mcp.cli")
)

# The service's own modules are imported by name in a couple of places (rerank backends, extractors).
hiddenimports += collect_submodules("pai_rag_service")

analysis = Analysis(
    ["entry.py"],
    pathex=[],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    # Nothing here draws a window or trains a model. Excluding them keeps the bundle from doubling.
    excludes=["tkinter", "matplotlib", "IPython", "notebook", "torch", "PySide6", "PyQt5"],
    noarchive=False,
)
pyz = PYZ(analysis.pure)

exe = EXE(
    pyz,
    analysis.scripts,
    [],
    exclude_binaries=True,
    name="pai-rag",
    debug=False,
    strip=False,
    upx=False,
    console=True,
)
collection = COLLECT(
    exe,
    analysis.binaries,
    analysis.datas,
    strip=False,
    upx=False,
    name="pai-rag",
)
