#!/usr/bin/env bash
# Build "Private AI.app" for macOS. Flags: --sign "<identity>", --dmg.
# With no --sign the signature is ad-hoc and the app runs only on this machine.
# Read packaging/README.md before signing for anyone else.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGING="${ROOT}/packaging"
PYTHON="${PYTHON:-${ROOT}/.venv/bin/python}"
APP_NAME="Private AI"
APP="${ROOT}/dist/${APP_NAME}.app"

SIGN_IDENTITY="${SIGN_IDENTITY:--}"
MAKE_DMG=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign) SIGN_IDENTITY="$2"; shift 2 ;;
    --dmg)  MAKE_DMG=1; shift ;;
    *) echo "Tham số không nhận ra: $1" >&2; exit 2 ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Script này chỉ chạy trên macOS." >&2
  exit 1
fi

if [[ ! -x "${PYTHON}" ]]; then
  echo "Không tìm thấy Python của môi trường ảo tại ${PYTHON}." >&2
  exit 1
fi

if ! "${PYTHON}" -c "import PyInstaller" >/dev/null 2>&1; then
  echo "Thiếu PyInstaller. Cài bằng: ${PYTHON} -m pip install 'private-ai[package]'" >&2
  exit 1
fi

echo "==> Dọn thư mục build cũ"
# Spotlight indexes a freshly signed bundle immediately, which makes rm fail with "Directory not empty"; one retry is enough.
for attempt in 1 2 3; do
  if rm -rf "${ROOT}/build" "${ROOT}/dist" 2>/dev/null; then break; fi
  [[ "${attempt}" == "3" ]] && { echo "Không xoá được ${ROOT}/dist." >&2; exit 1; }
  sleep 2
done

echo "==> Dựng icon từ brand mark"
"${PYTHON}" "${PACKAGING}/make_icon.py" "${PACKAGING}/PrivateAI.icns" >/dev/null

echo "==> PyInstaller"
( cd "${ROOT}" && "${PYTHON}" -m PyInstaller --noconfirm --clean "${PACKAGING}/private_ai.spec" )

if [[ ! -d "${APP}" ]]; then
  echo "PyInstaller không tạo ra ${APP}." >&2
  exit 1
fi

# Anything removed has to go before signing: deleting a file afterwards invalidates the signature.
echo "==> Cắt bớt phần không dùng"
FRAMEWORKS="${APP}/Contents/Frameworks"
for junk in \
  "${FRAMEWORKS}/PySide6/Qt/plugins/sqldrivers" \
  "${FRAMEWORKS}/PySide6/Qt/plugins/webview" \
  "${FRAMEWORKS}/PySide6/Qt/plugins/designer" \
  "${FRAMEWORKS}/PySide6/Qt/qml" \
  "${FRAMEWORKS}/PySide6/Qt/translations" \
  "${FRAMEWORKS}/PySide6/examples" \
  "${FRAMEWORKS}/PySide6/scripts" \
  "${FRAMEWORKS}/PySide6/glue" \
  "${FRAMEWORKS}/PySide6/typesystems" \
  "${FRAMEWORKS}/PySide6/include" \
  "${FRAMEWORKS}/PySide6/Assistant.app" \
  "${FRAMEWORKS}/PySide6/Designer.app" \
  "${FRAMEWORKS}/PySide6/Linguist.app" \
  ; do
  [[ -e "${junk}" ]] && rm -rf "${junk}" && echo "    bỏ $(basename "${junk}")"
done
# Compiled bytecode of files we do not ship the source of, and stray test suites.
find "${FRAMEWORKS}" -type d -name "__pycache__" -prune -exec rm -rf {} + 2>/dev/null || true
find "${FRAMEWORKS}" -type d -name "tests" -path "*/numpy/*" -prune -exec rm -rf {} + 2>/dev/null || true

# The compiled transcribe.cpp runtime, before signing because install_name_tool voids signatures; a build machine without it just ships no dictation.
echo "==> Nhúng ASR native"
"${PYTHON}" "${PACKAGING}/bundle_asr.py" "${APP}" || \
  echo "    bỏ qua: bản đóng gói này sẽ không có nhận dạng giọng nói" >&2

# Info.plist promises a minimum macOS while every Mach-O states the real one; a too-low promise makes dyld kill the app, so the number is measured, never typed.
echo "==> Đo phiên bản macOS tối thiểu thật"
FLOOR="$(find "${APP}/Contents" -type f \( -name '*.so' -o -name '*.dylib' -o -name 'private-ai' \) -print0 \
  | xargs -0 -n 1 otool -l 2>/dev/null \
  | grep -o 'minos [0-9.]*' | awk '{print $2}' | sort -V | tail -1)"
if [[ -n "${FLOOR}" ]]; then
  plutil -replace LSMinimumSystemVersion -string "${FLOOR}" "${APP}/Contents/Info.plist"
  echo "    macOS ${FLOOR} trở lên"
else
  echo "    không đo được, giữ nguyên giá trị trong spec" >&2
fi

echo "==> Ký"
if [[ "${SIGN_IDENTITY}" == "-" ]]; then
  echo "    chữ ký ad-hoc — app chỉ chạy trên máy này"
  codesign --force --deep --sign - \
    --entitlements "${PACKAGING}/entitlements.plist" \
    "${APP}"
else
  echo "    ${SIGN_IDENTITY}"
  # --deep is deprecated for real identities: sign nested code first, then the bundle.
  find "${APP}/Contents" \( -name "*.so" -o -name "*.dylib" \) -print0 \
    | xargs -0 -n 32 codesign --force --timestamp --options runtime \
        --entitlements "${PACKAGING}/entitlements.plist" --sign "${SIGN_IDENTITY}"
  find "${APP}/Contents/Frameworks" -maxdepth 1 -name "*.framework" -print0 2>/dev/null \
    | xargs -0 -r -n 1 codesign --force --timestamp --options runtime --sign "${SIGN_IDENTITY}"
  codesign --force --timestamp --options runtime \
    --entitlements "${PACKAGING}/entitlements.plist" \
    --sign "${SIGN_IDENTITY}" "${APP}"
fi

codesign --verify --deep --strict --verbose=2 "${APP}" 2>&1 | tail -3

# Replacing a bundle in place leaves LaunchServices holding the old registration, so the next `open` can launch the previous build; re-register to avoid that.
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
[[ -x "${LSREGISTER}" ]] && "${LSREGISTER}" -f "${APP}" || true

if [[ "${MAKE_DMG}" == "1" ]]; then
  echo "==> DMG"
  DMG="${ROOT}/dist/${APP_NAME}.dmg"
  STAGING="$(mktemp -d)"
  cp -R "${APP}" "${STAGING}/"
  ln -s /Applications "${STAGING}/Applications"
  hdiutil create -volname "${APP_NAME}" -srcfolder "${STAGING}" -ov -format UDZO "${DMG}" >/dev/null
  rm -rf "${STAGING}"
  echo "    ${DMG}"
fi

echo
echo "Xong: ${APP}"
du -sh "${APP}"
