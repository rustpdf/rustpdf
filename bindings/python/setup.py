"""Build hook for the rustpdf wheel.

rustpdf is a ``ctypes`` binding: it loads the prebuilt ``libpdf_ffi`` cdylib at
runtime and never links against libpython. So the wheel is *platform*-specific
(it ships a native binary) but *ABI*-agnostic across CPython versions — hence
the ``py3-none-<platform>`` tag forced below.

The native library is located, in order:

1. already inside ``rustpdf/`` (CI pre-builds it per-platform and drops it in);
2. ``$RUSTPDF_LIB`` (explicit path to a prebuilt cdylib);
3. the workspace build tree (``target/release`` then ``target/debug``);
4. otherwise ``cargo build -p pdf-ffi --release`` is invoked (local installs).

It is then copied next to ``rustpdf/__init__.py`` so the wheel bundles it.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

from setuptools import setup
from setuptools.command.build_py import build_py as _build_py

try:
    from wheel.bdist_wheel import bdist_wheel as _bdist_wheel
except ImportError:  # pragma: no cover - wheel always present during a wheel build
    _bdist_wheel = None

HERE = Path(__file__).resolve().parent
PACKAGE = HERE / "rustpdf"
REPO_ROOT = HERE.parents[1]  # bindings/python -> bindings -> repo root


def _lib_name() -> str:
    if sys.platform == "darwin":
        return "libpdf_ffi.dylib"
    if sys.platform == "win32":
        return "pdf_ffi.dll"
    return "libpdf_ffi.so"


def _find_prebuilt(name: str) -> Path | None:
    if (PACKAGE / name).is_file():
        return PACKAGE / name
    if env := os.environ.get("RUSTPDF_LIB"):
        p = Path(env)
        return p if p.is_file() else None
    for profile in ("release", "debug"):
        candidate = REPO_ROOT / "target" / profile / name
        if candidate.is_file():
            return candidate
    return None


def ensure_native_lib() -> None:
    """Make sure ``rustpdf/<libname>`` exists, building the cdylib if needed."""
    name = _lib_name()
    target = PACKAGE / name
    found = _find_prebuilt(name)

    if found is None:
        # No prebuilt artifact anywhere — compile it (local `pip install .`).
        print(f"rustpdf: building native library via cargo ({name})", flush=True)
        subprocess.run(
            ["cargo", "build", "-p", "pdf-ffi", "--release"],
            cwd=REPO_ROOT,
            check=True,
        )
        found = REPO_ROOT / "target" / "release" / name

    if found.resolve() != target.resolve():
        shutil.copy2(found, target)
    print(f"rustpdf: bundling {target.name} from {found}", flush=True)


class build_py(_build_py):
    def run(self) -> None:
        ensure_native_lib()
        super().run()


if _bdist_wheel is not None:

    class bdist_wheel(_bdist_wheel):
        def finalize_options(self) -> None:
            super().finalize_options()
            # Contains a native binary -> not a pure-Python wheel; gets a platform tag.
            self.root_is_pure = False

        def get_tag(self):
            # ctypes binding: works on any CPython 3, so override the interpreter/ABI
            # tags to the universal "py3-none", keeping only the platform tag.
            _python, _abi, plat = super().get_tag()
            return "py3", "none", plat

    cmdclass = {"build_py": build_py, "bdist_wheel": bdist_wheel}
else:  # pragma: no cover
    cmdclass = {"build_py": build_py}


setup(cmdclass=cmdclass)
