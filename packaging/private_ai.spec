# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller recipe for the macOS bundle.
The excludes are the build: PySide6 ships 1.2 GB of Qt and this app imports five modules.
Some imports are invisible to static analysis, so they are collected or listed by hand."""

from pathlib import Path

from PyInstaller.utils.hooks import collect_all, collect_submodules, copy_metadata

ROOT = Path(SPECPATH).resolve().parent
SRC = ROOT / "src"

APP_NAME = "Private AI"
BUNDLE_IDENTIFIER = "com.vinhpx.private-ai"
AUTHOR = "vinhpx"
VERSION = "0.2.0"

# Data the app reads off disk at runtime; missing either silently loses a feature - no fonts, or no skills at all.
datas = [
    (str(SRC / "private_ai" / "ui" / "assets"), "private_ai/ui/assets"),
    (str(SRC / "private_ai" / "agent" / "skills" / "builtin"), "private_ai/agent/skills/builtin"),
]
binaries = []

# The app's own modules, all of them: two registries name modules as strings and swallow ImportError, so a half-collected build starts with placeholder screens and no tools.
hiddenimports = collect_submodules("private_ai")

# Packages whose contents cannot be found by following imports: plugin registries, lazily named modules, data files beside the code.
COLLECT_WHOLE = (
    "lightrag",
    "markitdown",
    "markitdown_ocr",
    "tiktoken",
    "tiktoken_ext",
    "docx",
    "pptx",
    "langchain",
    "langchain_core",
    "langchain_text_splitters",
    "langchain_openai",
    "langchain_ollama",
    "langgraph",
    "mcp",
)
for package in COLLECT_WHOLE:
    try:
        package_datas, package_binaries, package_hidden = collect_all(package)
    except Exception:  # noqa: BLE001 - an optional package simply is not bundled
        continue
    datas += package_datas
    binaries += package_binaries
    hiddenimports += package_hidden

# Distribution metadata: several of these read their own version through `importlib.metadata` at import time and raise without it.
for distribution in ("markitdown", "lightrag-hku", "langchain", "langchain-core", "openai", "mcp"):
    try:
        datas += copy_metadata(distribution, recursive=True)
    except Exception:  # noqa: BLE001 - not installed, nothing to copy
        pass

# tiktoken names this module as a string when it looks for BPE encodings.
hiddenimports += ["tiktoken_ext.openai_public", "tiktoken_ext"]

# Every Qt module the app does not import; QtWebEngineCore is the expensive one, the rest are listed so an accidental dependency fails loudly at build time.
UNUSED_QT = (
    "Qt3DAnimation", "Qt3DCore", "Qt3DExtras", "Qt3DInput", "Qt3DLogic", "Qt3DRender",
    "QtBluetooth", "QtCanvasPainter", "QtCharts", "QtDataVisualization", "QtDesigner",
    "QtGraphs", "QtGraphsWidgets", "QtHelp", "QtHttpServer", "QtLocation", "QtNetworkAuth",
    "QtNfc", "QtPdf", "QtPdfWidgets", "QtPositioning", "QtQml", "QtQuick", "QtQuick3D",
    "QtQuickControls2", "QtQuickTest", "QtQuickWidgets", "QtRemoteObjects", "QtScxml",
    "QtSensors", "QtSerialBus", "QtSerialPort", "QtSpatialAudio", "QtSql", "QtStateMachine",
    "QtTest", "QtTextToSpeech", "QtUiTools", "QtWebChannel", "QtWebEngineCore",
    "QtWebEngineQuick", "QtWebEngineWidgets", "QtWebSockets", "QtWebView",
)

excludes = [f"PySide6.{name}" for name in UNUSED_QT]
excludes += [
    # Test frameworks and dev tooling that dependencies import behind try/except.
    "pytest",
    "_pytest",
    "ruff",
    "IPython",
    "jupyter",
    "notebook",
    # Plotting and notebook stacks some data libraries import optionally.
    "matplotlib",
    "tkinter",
    "PyQt5",
    "PyQt6",
    "PySide2",
]

analysis = Analysis(
    [str(ROOT / "packaging" / "entry.py")],
    pathex=[str(SRC)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=excludes,
    noarchive=False,
    optimize=0,
)

pyz = PYZ(analysis.pure)

executable = EXE(
    pyz,
    analysis.scripts,
    [],
    exclude_binaries=True,
    name="private-ai",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

collection = COLLECT(
    executable,
    analysis.binaries,
    analysis.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="private-ai",
)

app = BUNDLE(
    collection,
    name=f"{APP_NAME}.app",
    icon=str(ROOT / "packaging" / "PrivateAI.icns"),
    bundle_identifier=BUNDLE_IDENTIFIER,
    version=VERSION,
    info_plist={
        "CFBundleName": APP_NAME,
        "CFBundleDisplayName": APP_NAME,
        "CFBundleShortVersionString": VERSION,
        "CFBundleVersion": VERSION,
        "NSHumanReadableCopyright": f"© 2026 {AUTHOR}",
        "LSApplicationCategoryType": "public.app-category.productivity",
        # A floor, replaced by build.sh with the highest `minos` any Mach-O actually requires; a typed number is how a bundle promises macOS 12 and dies on launch.
        "LSMinimumSystemVersion": "12.0",
        "NSHighResolutionCapable": True,
        # Without this macOS forces the light appearance and the dark theme renders in a light frame.
        "NSRequiresAquaSystemAppearance": False,
        # The prompt shown the first time dictation is used; macOS denies the microphone outright, with no dialog, when this key is absent.
        "NSMicrophoneUsageDescription": (
            "Private AI dùng micro để chuyển giọng nói thành văn bản. "
            "Âm thanh được xử lý ngay trên máy này và không gửi đi đâu cả."
        ),
        "NSDocumentsFolderUsageDescription": (
            "Private AI đọc tài liệu bạn chọn để lập chỉ mục cho việc tìm kiếm."
        ),
        "NSDownloadsFolderUsageDescription": (
            "Private AI đọc tài liệu bạn chọn để lập chỉ mục cho việc tìm kiếm."
        ),
    },
)
