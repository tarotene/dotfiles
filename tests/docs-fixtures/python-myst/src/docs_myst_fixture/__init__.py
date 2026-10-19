"""Fixture package for the docs-python action with a `docs` extra."""


def add(a: int, b: int) -> int:
    """Add two numbers.

    >>> add(1, 2)
    3
    """
    return a + b
