#!/usr/bin/env python3
"""Evaluate Cura formulas with Uranium's SettingFunction, without Qt.

SettingFunction is LGPL-3.0 and lives in the Cura AppImage. This harness
loads that file from ``SHALE_URANIUM_ROOT`` (the extracted AppImage root).
It does not copy Uranium into the repository, and it does not import the
``UM`` package, because that package pulls in PyQt6.

Usage:
    SHALE_URANIUM_ROOT=/path/to/squashfs-root \\
        python3 scripts/formula_oracle.py resolved.json formulas.json

``resolved.json`` is an object of setting key to the string Shale stored.
``formulas.json`` is a list of ``{"key", "expr"}``. The script prints a
JSON list of ``{"key", "shale", "cura", "match"}``.
"""

from __future__ import annotations

import importlib.util
import json
import math
import sys
import types
from pathlib import Path


def load_setting_function(root: Path):
    """Load SettingFunction.py without executing UM/__init__.py."""
    source = root / "UM" / "Settings" / "SettingFunction.py"
    if not source.is_file():
        raise SystemExit(f"SettingFunction.py not found under {root}")

    def module(name: str) -> types.ModuleType:
        existing = sys.modules.get(name)
        if existing is not None:
            return existing
        created = types.ModuleType(name)
        created.__path__ = []  # type: ignore[attr-defined]
        sys.modules[name] = created
        return created

    um = module("UM")
    settings = module("UM.Settings")
    logger_mod = module("UM.Logger")

    class Logger:
        @staticmethod
        def log(*_args, **_kwargs) -> None:
            return None

        @staticmethod
        def logException(*_args, **_kwargs) -> None:
            return None

    logger_mod.Logger = Logger
    um.Logger = Logger

    context_mod = module("UM.Settings.PropertyEvaluationContext")

    class PropertyEvaluationContext:
        def __init__(self, source_stack=None) -> None:
            self.context = {}
            self.source_stack = source_stack

        def rootStack(self):
            return self.source_stack

    context_mod.PropertyEvaluationContext = PropertyEvaluationContext

    interfaces = module("UM.Settings.Interfaces")

    class ContainerInterface:
        pass

    interfaces.ContainerInterface = ContainerInterface
    settings.Interfaces = interfaces
    settings.PropertyEvaluationContext = context_mod

    spec = importlib.util.spec_from_file_location("shale_setting_function", source)
    if spec is None or spec.loader is None:
        raise SystemExit(f"could not load {source}")
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded.SettingFunction


def python_value(text: str):
    if text == "true":
        return True
    if text == "false":
        return False
    try:
        if text.lower() in {"inf", "+inf", "infinity"}:
            return math.inf
        if text.lower() in {"-inf", "-infinity"}:
            return -math.inf
        number = float(text)
    except ValueError:
        return text
    if number.is_integer() and abs(number) < 1e15:
        return int(number)
    return number


def same(cura, shale: str) -> bool:
    if cura is None:
        return False
    if isinstance(cura, bool):
        return shale == ("true" if cura else "false")
    if isinstance(cura, (int, float)):
        try:
            return abs(float(shale) - float(cura)) <= 1e-6 * max(1.0, abs(float(cura)))
        except ValueError:
            return False
    if isinstance(cura, str):
        return shale == cura
    if isinstance(cura, (list, tuple)):
        return shale.replace(" ", "") == json.dumps(list(cura)).replace(" ", "")
    return str(cura) == shale


class Provider:
    def __init__(self, values: dict[str, str]) -> None:
        self.values = values

    def getProperty(self, key, _property, _context=None):
        if key not in self.values:
            return None
        return python_value(self.values[key])


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: formula_oracle.py resolved.json formulas.json")
    root = Path(
        __import__("os").environ.get("SHALE_URANIUM_ROOT", "")
    )
    if not root:
        raise SystemExit("set SHALE_URANIUM_ROOT to the extracted Cura AppImage")
    setting_function = load_setting_function(root)
    values = json.loads(Path(sys.argv[1]).read_text())
    formulas = json.loads(Path(sys.argv[2]).read_text())
    provider = Provider(values)

    def extruder_value(_index, key):
        return provider.getProperty(key, "value")

    def extruder_values(key):
        value = provider.getProperty(key, "value")
        return [] if value is None else [value]

    def resolve_or_value(key):
        return provider.getProperty(key, "value")

    def any_extruder_with_material(_key):
        return -1

    def default_extruder_position():
        return 0

    def extruder_value_from_container(_index, key, _container_index):
        return provider.getProperty(key, "value")

    setting_function.registerOperator("extruderValue", extruder_value)
    setting_function.registerOperator("extruderValues", extruder_values)
    setting_function.registerOperator("resolveOrValue", resolve_or_value)
    setting_function.registerOperator("anyExtruderWithMaterial", any_extruder_with_material)
    setting_function.registerOperator("defaultExtruderPosition", default_extruder_position)
    setting_function.registerOperator("extruderValueFromContainer", extruder_value_from_container)

    report = []
    for item in formulas:
        expr = item["expr"]
        function = setting_function(expr)
        if not function.isValid():
            report.append({"key": item["key"], "shale": values.get(item["key"]), "cura": None, "match": False, "invalid": True})
            continue
        try:
            cura = function(provider)
        except Exception as err:  # SettingFunction usually swallows these
            report.append({"key": item["key"], "shale": values.get(item["key"]), "cura": None, "match": False, "error": str(err)})
            continue
        shale = values.get(item["key"], "")
        report.append({"key": item["key"], "shale": shale, "cura": cura, "match": same(cura, shale)})
    json.dump(report, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
