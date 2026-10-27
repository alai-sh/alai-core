# Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
# MIT License (see LICENSE or https://opensource.org/licenses/MIT)

"""What the alai tools share, such as the index format, collection metadata, plugin indexes and extraction, and the
codecs they're read with. Nothing is imported here, so importing one part never loads another, or the native extension."""


def __getattr__(name: str) -> str:
    # Looked up and cached on first use.
    if name == "__version__":
        from importlib.metadata import version

        value = version("alai-core")
        globals()["__version__"] = value
        return value
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
