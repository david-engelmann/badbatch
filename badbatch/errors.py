"""Error types for badbatch."""


class BadbatchError(Exception):
    """Base class for every refusal badbatch raises."""


class RecordError(BadbatchError):
    """A recorded run record is malformed."""


class StampError(BadbatchError):
    """A stamp document is malformed."""


class StateError(BadbatchError):
    """A recorded current state is malformed."""


class WalkError(BadbatchError):
    """A recall walk cannot be started as asked."""
