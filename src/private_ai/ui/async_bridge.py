"""The one place a Qt slot is allowed to touch asyncio.

Two failure modes this exists to prevent. First, ``asyncio.create_task`` returns a task the
loop holds only weakly: drop the reference and the garbage collector can kill a half-run
service call, which shows up as a request that silently never completes. Second, an
exception inside a fire-and-forget task is logged by the loop's default handler at process
exit and nowhere else — from the user's side the button simply did nothing. Everything
here is about making both impossible by construction.

Never call ``asyncio.run`` or ``loop.run_until_complete`` from a slot: the qasync loop is
already running and re-entering it deadlocks the UI.
"""

from __future__ import annotations

import asyncio
import functools
import logging
from typing import TYPE_CHECKING, Any

from PySide6.QtCore import QObject, Signal

if TYPE_CHECKING:  # pragma: no cover - import graph only
    from collections.abc import Callable, Coroutine

logger = logging.getLogger("private_ai.ui.async")

__all__ = ["AsyncBridge", "bridge", "cancel_all", "run_coro", "set_toast_sink", "slot_async"]

FAILED = "Thao tác không thành công"


class AsyncBridge(QObject):
    """Owns the in-flight tasks and turns unhandled failures into a signal.

    Not a singleton by design — tests build their own — but the module-level ``bridge()``
    is what the app uses so a widget never has to be handed one.
    """

    failed = Signal(str, object)  # (message, exception)

    def __init__(self, parent: QObject | None = None) -> None:
        super().__init__(parent)
        self._tasks: set[asyncio.Task[Any]] = set()
        self._toast: Callable[[str, str], None] | None = None

    def set_toast_sink(self, sink: Callable[[str, str], None] | None) -> None:
        self._toast = sink

    def pending(self) -> int:
        return len(self._tasks)

    def run(
        self,
        coro: Coroutine[Any, Any, Any],
        *,
        on_result: Callable[[Any], None] | None = None,
        on_error: Callable[[BaseException], None] | None = None,
        owner: QObject | None = None,
        label: str = "",
    ) -> asyncio.Task[Any]:
        # ``get_running_loop`` is right once the app is spinning; during window
        # construction there is a loop set but not yet running, and that is the case the
        # fallback covers.
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            try:
                loop = asyncio.get_event_loop_policy().get_event_loop()
            except RuntimeError:  # pragma: no cover - only outside a qasync app
                coro.close()
                raise

        task = loop.create_task(coro)
        # The strong reference that keeps the task alive; discarded in the callback.
        self._tasks.add(task)
        if owner is not None:
            # A widget that is destroyed mid-call cannot receive the result, and the call
            # is usually only interesting because that widget wanted it.
            owner.destroyed.connect(lambda *_: task.cancel())
        task.add_done_callback(
            functools.partial(self._finished, on_result=on_result, on_error=on_error, label=label)
        )
        return task

    def _finished(
        self,
        task: asyncio.Task[Any],
        *,
        on_result: Callable[[Any], None] | None,
        on_error: Callable[[BaseException], None] | None,
        label: str,
    ) -> None:
        self._tasks.discard(task)
        if task.cancelled():
            return
        error = task.exception()
        if error is None:
            if on_result is not None:
                # A crash in the callback must not be swallowed by the done-callback
                # machinery, which would log it nowhere the user can see.
                try:
                    on_result(task.result())
                except Exception as cause:  # noqa: BLE001 - surfaced, not hidden
                    self._report(cause, label)
            return
        if on_error is not None:
            on_error(error)
            return
        self._report(error, label)

    def _report(self, error: BaseException, label: str = "") -> None:
        where = f" ({label})" if label else ""
        logger.exception("Tác vụ nền lỗi%s", where, exc_info=error)
        message = _message(error)
        self.failed.emit(message, error)
        if self._toast is not None:
            self._toast(message, "error")

    def cancel_all(self) -> None:
        for task in list(self._tasks):
            task.cancel()
        self._tasks.clear()


def _message(error: BaseException) -> str:
    text = str(error).strip()
    if text:
        return text if len(text) <= 240 else f"{text[:239]}…"
    return f"{FAILED}: {type(error).__name__}"


_bridge: AsyncBridge | None = None


def bridge() -> AsyncBridge:
    global _bridge
    if _bridge is None:
        _bridge = AsyncBridge()
    return _bridge


def set_toast_sink(sink: Callable[[str, str], None] | None) -> None:
    """Wired once by ``MainWindow`` so every unowned failure lands in the toast stack."""
    bridge().set_toast_sink(sink)


def run_coro(
    coro: Coroutine[Any, Any, Any],
    on_result: Callable[[Any], None] | None = None,
    on_error: Callable[[BaseException], None] | None = None,
    *,
    owner: QObject | None = None,
    label: str = "",
) -> asyncio.Task[Any]:
    return bridge().run(coro, on_result=on_result, on_error=on_error, owner=owner, label=label)


def cancel_all() -> None:
    bridge().cancel_all()


def slot_async(func: Callable[..., Coroutine[Any, Any, Any]]) -> Callable[..., asyncio.Task[Any]]:
    """Make an ``async def`` connectable to a Qt signal.

    Qt calls the wrapper synchronously and gets a task back; the coroutine runs on the
    qasync loop with the same guarantees as ``run_coro``. Slots receive whatever arguments
    the signal carries, so the wrapper forwards them untouched.
    """

    @functools.wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> asyncio.Task[Any]:
        owner = args[0] if args and isinstance(args[0], QObject) else None
        return run_coro(func(*args, **kwargs), owner=owner, label=func.__qualname__)

    return wrapper
