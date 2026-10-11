"""Transport primitives only; no production project or execution API yet."""
from .framing import read_frame, write_frame
__all__ = ["read_frame", "write_frame"]
