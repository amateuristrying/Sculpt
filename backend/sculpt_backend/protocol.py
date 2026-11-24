"""Standard versioned worker protocol for local NDJSON IPC.

All inference libraries output diagnostics to stderr.
Stdout is strictly reserved for single-line NDJSON protocol envelopes.
"""
from __future__ import annotations

import json
from pathlib import Path
import sys
from typing import Any, Dict, List, Optional, Union

PROTOCOL_VERSION: int = 1

VALID_STAGES: set[str] = {
    "analyzing",
    "loading",
    "geometry",
    "surface",
    "preparing",
}


def format_envelope(kind: str, job_id: Optional[str] = None, **fields: Any) -> str:
    """Format a single NDJSON protocol envelope."""
    payload: Dict[str, Any] = {
        "protocol": PROTOCOL_VERSION,
        "type": kind,
    }
    if job_id is not None:
        payload["job_id"] = str(job_id)
    payload.update(fields)
    return json.dumps(payload)


def parse_envelope(line: str) -> Dict[str, Any]:
    """Parse and validate an NDJSON protocol envelope line."""
    line_clean = line.strip()
    if not line_clean:
        raise ValueError("Empty protocol line")

    try:
        data = json.loads(line_clean)
    except json.JSONDecodeError as exc:
        raise ValueError(f"Malformed JSON in protocol envelope: {exc}") from exc

    if not isinstance(data, dict):
        raise ValueError("Protocol envelope must be a JSON object")

    # Validate protocol version (supports 'protocol' or 'protocol_version')
    protocol_version = data.get("protocol")
    if protocol_version is None:
        protocol_version = data.get("protocol_version")
    if protocol_version is None:
        raise ValueError("Missing protocol version in envelope")
    if not isinstance(protocol_version, int) or protocol_version != PROTOCOL_VERSION:
        raise ValueError(
            f"Unsupported protocol version {protocol_version} (expected {PROTOCOL_VERSION})"
        )

    # Validate event type (supports 'type' or 'event')
    event_type = data.get("type") or data.get("event")
    if not event_type or not isinstance(event_type, str):
        raise ValueError("Missing event type in protocol envelope")

    if event_type == "stage":
        stage = data.get("stage")
        if not stage or stage not in VALID_STAGES:
            raise ValueError(f"Invalid or missing stage in stage event: '{stage}'")
    elif event_type == "progress":
        stage = data.get("stage")
        if not stage or stage not in VALID_STAGES:
            raise ValueError(f"Invalid or missing stage in progress event: '{stage}'")
        if "progress" not in data or not isinstance(data["progress"], (int, float)):
            raise ValueError("Missing or invalid numeric progress value")
    elif event_type in ("warning", "error"):
        if "message" not in data or not isinstance(data["message"], str):
            raise ValueError(f"Missing message in {event_type} event")
    elif event_type in ("accepted", "result", "cancelled", "probe"):
        pass
    else:
        raise ValueError(f"Unknown protocol event type: '{event_type}'")

    return data


def emit(kind: str, job_id: Optional[str] = None, stream=None, **fields: Any) -> None:
    """Emit a raw event envelope to the given stream (default stdout)."""
    out = stream if stream is not None else sys.stdout
    print(format_envelope(kind, job_id=job_id, **fields), file=out, flush=True)


def progress(stage: str, percent: float, message: str, job_id: Optional[str] = None, stream=None) -> None:
    """Emit a progress event (backward compatible with setup and worker)."""
    emit("progress", job_id=job_id, stream=stream, stage=stage, progress=percent, message=message)


def emit_accepted(job_id: Optional[str] = None, message: str = "Request accepted", stream=None) -> None:
    """Acknowledge receipt and acceptance of a job request."""
    emit("accepted", job_id=job_id, stream=stream, message=message)


def emit_stage(stage: str, job_id: Optional[str] = None, message: Optional[str] = None, stream=None) -> None:
    """Emit an inference stage transition event."""
    if stage not in VALID_STAGES:
        raise ValueError(f"Unknown inference stage: '{stage}'")
    fields: Dict[str, Any] = {"stage": stage}
    if message is not None:
        fields["message"] = message
    emit("stage", job_id=job_id, stream=stream, **fields)


def emit_progress(stage: str, progress: float, message: str, job_id: Optional[str] = None, stream=None) -> None:
    """Emit a structured progress event."""
    if stage not in VALID_STAGES:
        raise ValueError(f"Unknown inference stage: '{stage}'")
    emit("progress", job_id=job_id, stream=stream, stage=stage, progress=float(progress), message=message)


def emit_warning(message: str, code: Optional[str] = None, job_id: Optional[str] = None, stream=None) -> None:
    """Emit a non-fatal warning event."""
    fields: Dict[str, Any] = {"message": message}
    if code is not None:
        fields["code"] = code
    emit("warning", job_id=job_id, stream=stream, **fields)


def emit_result(
    metrics: Optional[Dict[str, Any]] = None,
    artifacts: Optional[List[Union[str, Path]]] = None,
    duration_ms: Optional[int] = None,
    peak_memory_mb: Optional[float] = None,
    job_id: Optional[str] = None,
    stream=None,
) -> None:
    """Emit a result manifest event containing metrics and generated artifacts."""
    fields: Dict[str, Any] = {}
    if metrics is not None:
        fields["metrics"] = metrics
    if artifacts:
        fields["artifacts"] = [str(p) for p in artifacts]
    if duration_ms is not None:
        fields["duration_ms"] = int(duration_ms)
    if peak_memory_mb is not None:
        fields["peak_memory_mb"] = float(peak_memory_mb)
    emit("result", job_id=job_id, stream=stream, **fields)


def emit_error(message: str, code: Optional[str] = None, job_id: Optional[str] = None, stream=None) -> None:
    """Emit a fatal structured error event."""
    fields: Dict[str, Any] = {"message": message}
    if code is not None:
        fields["code"] = code
    emit("error", job_id=job_id, stream=stream, **fields)


def emit_cancelled(message: str = "Job cancelled", job_id: Optional[str] = None, stream=None) -> None:
    """Acknowledge cancellation of a job."""
    emit("cancelled", job_id=job_id, stream=stream, message=message)


def validate_output_path(candidate: Union[str, Path], workspace_dir: Union[str, Path]) -> Path:
    """Validate that candidate output path is strictly contained within workspace_dir.

    Rejects path traversal ('..') and absolute paths pointing outside workspace.
    """
    cand = Path(candidate)
    ws = Path(workspace_dir).resolve()

    for part in cand.parts:
        if part == "..":
            raise ValueError(f"Path traversal forbidden: '{candidate}'")

    if cand.is_absolute():
        resolved = cand.resolve()
    else:
        resolved = (ws / cand).resolve()

    try:
        resolved.relative_to(ws)
    except ValueError as exc:
        raise ValueError(
            f"Output path '{candidate}' escapes job workspace '{workspace_dir}'"
        ) from exc

    return resolved
