"""The five screens the sidebar switches between.

Nothing is re-exported here on purpose: ``MainWindow`` imports each view module by path
and replaces a module that fails to import with a placeholder, so a package-level import
of all five would turn one broken view into a dead application.
"""

from __future__ import annotations
