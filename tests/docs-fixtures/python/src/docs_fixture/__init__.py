"""Fixture package for the docs-python composite action."""

from pathlib import Path


def add(a: int, b: int) -> int:
    """Add two numbers.

    >>> add(1, 2)
    3
    """
    return a + b


def name_of(path: Path) -> str:
    """Return the final component of ``path``.

    A standard-library type in the annotation must resolve under nitpicky mode.
    """
    return path.name
