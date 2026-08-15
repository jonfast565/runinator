"""the handlers `runinator-function.json` declares.

each export is a plain function taking the declared input as keyword arguments and returning a dict
matching its declared output. nothing here imports a runinator library: a packaged function is
ordinary code, and the sandbox is what connects it to a run.
"""


def resize(source: str, width: int) -> dict:
    """resize an image to `width`, preserving aspect ratio."""
    original_width, original_height, _ = _probe(source)
    height = max(1, round(original_height * (width / original_width)))
    return {"uri": _write(source, width, height), "width": width, "height": height}


def inspect(source: str) -> dict:
    """report an image's dimensions and format without modifying it."""
    width, height, image_format = _probe(source)
    return {"width": width, "height": height, "format": image_format}


def _probe(source: str) -> tuple:
    # placeholder for a real decoder; the example ships without third-party dependencies so it can
    # be published and executed from a checkout with nothing installed.
    del source
    return 1920, 1080, "png"


def _write(source: str, width: int, height: int) -> str:
    return f"{source}?w={width}&h={height}"
